//! Bounded execution primitives for DREAD.
//!
//! This crate does not discover providers, execute plugins, or own GPU resources.
//! Callers supply an explicitly selected runner. The existing Python GPU Supervisor
//! remains the sole authority for GPU admission and lifecycle.

use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use serde_json::Value;
use tokio::sync::{Semaphore, watch};

const CANCELLATION_CLEANUP_GRACE: Duration = Duration::from_secs(5);

/// Application-level admission classes. GPU inference permits only bound request
/// concurrency; the Python GPU Supervisor remains the sole hardware/lease authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionResourceClass {
    General,
    Cpu,
    Network,
    Agent,
    GpuInference,
    Media,
}

/// Per-class concurrency ceilings layered underneath the global execution limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub cpu: usize,
    pub network: usize,
    pub agent: usize,
    pub gpu_inference: usize,
    pub media: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu: 2,
            network: 2,
            agent: 2,
            gpu_inference: 1,
            media: 1,
        }
    }
}

impl ResourceLimits {
    fn capacity(self, class: ExecutionResourceClass, global: usize) -> usize {
        match class {
            ExecutionResourceClass::General => global,
            ExecutionResourceClass::Cpu => self.cpu,
            ExecutionResourceClass::Network => self.network,
            ExecutionResourceClass::Agent => self.agent,
            ExecutionResourceClass::GpuInference => self.gpu_inference,
            ExecutionResourceClass::Media => self.media,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionConfig {
    pub max_concurrency: usize,
    pub max_admitted: usize,
    pub max_attempts: u8,
    pub attempt_timeout: Duration,
    /// Initial delay before a retry; zero disables retry waiting.
    pub retry_backoff: Duration,
    /// Upper bound for exponential retry delay.
    pub max_retry_backoff: Duration,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 2,
            max_admitted: 8,
            max_attempts: 1,
            attempt_timeout: Duration::from_secs(1805),
            retry_backoff: Duration::from_millis(250),
            max_retry_backoff: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    ZeroConcurrency,
    AdmissionBelowConcurrency,
    ZeroAttempts,
    ZeroTimeout,
    MaxRetryBackoffBelowInitial,
    ZeroResourceConcurrency { class: ExecutionResourceClass },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerError {
    pub message: String,
    pub retryable: bool,
}

impl RunnerError {
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }

    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    InvalidTaskId,
    InvalidTimeout,
    AdmissionFull { limit: usize },
    Cancelled { attempts: u8 },
    TimedOut { attempts: u8 },
    RunnerFailed { attempts: u8, message: String },
    SchedulerClosed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionResult {
    pub task_id: String,
    pub attempts: u8,
    pub result: Value,
    pub elapsed_ms: u64,
}

#[derive(Clone)]
pub struct ExecutionGate {
    semaphore: Arc<Semaphore>,
    resource_semaphores: Arc<HashMap<ExecutionResourceClass, Arc<Semaphore>>>,
    admitted: Arc<AtomicUsize>,
    config: ExecutionConfig,
}

/// Holds global and per-class permits for the lifetime of one operation.
pub struct ExecutionPermit {
    _admission: AdmissionGuard,
    _resource_permit: tokio::sync::OwnedSemaphorePermit,
    _global_permit: tokio::sync::OwnedSemaphorePermit,
}

struct AdmissionGuard {
    admitted: Arc<AtomicUsize>,
}

impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        self.admitted.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ExecutionGate {
    pub fn new(config: ExecutionConfig) -> Result<Self, ConfigError> {
        Self::new_with_resource_limits(config, ResourceLimits::default())
    }

    pub fn new_with_resource_limits(
        config: ExecutionConfig,
        limits: ResourceLimits,
    ) -> Result<Self, ConfigError> {
        if config.max_concurrency == 0 {
            return Err(ConfigError::ZeroConcurrency);
        }
        if config.max_admitted < config.max_concurrency {
            return Err(ConfigError::AdmissionBelowConcurrency);
        }
        if config.max_attempts == 0 {
            return Err(ConfigError::ZeroAttempts);
        }
        if config.attempt_timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout);
        }
        if config.max_retry_backoff < config.retry_backoff {
            return Err(ConfigError::MaxRetryBackoffBelowInitial);
        }

        let classes = [
            ExecutionResourceClass::General,
            ExecutionResourceClass::Cpu,
            ExecutionResourceClass::Network,
            ExecutionResourceClass::Agent,
            ExecutionResourceClass::GpuInference,
            ExecutionResourceClass::Media,
        ];
        let mut resource_semaphores = HashMap::with_capacity(classes.len());
        for class in classes {
            let capacity = limits.capacity(class, config.max_concurrency);
            if capacity == 0 {
                return Err(ConfigError::ZeroResourceConcurrency { class });
            }
            resource_semaphores.insert(class, Arc::new(Semaphore::new(capacity)));
        }

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            resource_semaphores: Arc::new(resource_semaphores),
            admitted: Arc::new(AtomicUsize::new(0)),
            config,
        })
    }

    pub fn admitted_count(&self) -> usize {
        self.admitted.load(Ordering::Acquire)
    }

    async fn wait_before_retry(
        &self,
        attempts: u8,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<(), ExecutionError> {
        if *cancellation.borrow() {
            return Err(ExecutionError::Cancelled { attempts });
        }
        if self.config.retry_backoff.is_zero() {
            return Ok(());
        }

        let shift = u32::from(attempts.saturating_sub(1)).min(31);
        let multiplier = 1u32 << shift;
        let delay = self
            .config
            .retry_backoff
            .checked_mul(multiplier)
            .unwrap_or(self.config.max_retry_backoff)
            .min(self.config.max_retry_backoff);

        loop {
            if *cancellation.borrow() {
                return Err(ExecutionError::Cancelled { attempts });
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => return Ok(()),
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        return Err(ExecutionError::Cancelled { attempts });
                    }
                }
            }
        }
    }

    fn try_admit(&self) -> Result<AdmissionGuard, ExecutionError> {
        self.admitted
            .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < self.config.max_admitted).then_some(current + 1)
            })
            .map_err(|_| ExecutionError::AdmissionFull {
                limit: self.config.max_admitted,
            })?;
        Ok(AdmissionGuard {
            admitted: Arc::clone(&self.admitted),
        })
    }

    /// Acquire a global and class-specific slot for work that has its own lifecycle.
    ///
    /// This is useful for provider adapters that poll for a long-running result rather
    /// than running inside the retrying closure. Cancellation while waiting releases
    /// any already-acquired permit and never starts the operation.
    pub async fn acquire_resource(
        &self,
        class: ExecutionResourceClass,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<ExecutionPermit, ExecutionError> {
        let admission = self.try_admit()?;
        let resource_semaphore = self
            .resource_semaphores
            .get(&class)
            .cloned()
            .ok_or(ExecutionError::SchedulerClosed)?;

        // Acquire the narrower class first so work waiting on a class quota does not
        // occupy a global worker slot needed by another resource class.
        let resource_permit = acquire_cancellable(resource_semaphore, cancellation).await?;
        let global_permit = acquire_cancellable(Arc::clone(&self.semaphore), cancellation).await?;

        Ok(ExecutionPermit {
            _admission: admission,
            _resource_permit: resource_permit,
            _global_permit: global_permit,
        })
    }

    /// Run a caller-supplied operation under bounded admission and the General quota.
    ///
    /// The watch sender cancels by setting true. A dropped sender also cancels.
    /// The operation receives a cloned receiver for cooperative cancellation.
    pub async fn run<F, Fut>(
        &self,
        task_id: impl Into<String>,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, String>> + Send,
    {
        self.run_with_resource_class(
            task_id,
            ExecutionResourceClass::General,
            cancellation,
            operation,
        )
        .await
    }

    /// Run a caller-supplied operation under its resource-class and global limits.
    pub async fn run_with_resource_class<F, Fut>(
        &self,
        task_id: impl Into<String>,
        class: ExecutionResourceClass,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, String>> + Send,
    {
        self.run_with_resource_class_and_timeout(task_id, class, None, cancellation, operation)
            .await
    }

    /// Run with an optional per-node timeout override. None uses the workspace default.
    /// Timed-out work is not replayed automatically because its side-effect outcome may
    /// be unknown; retries remain limited to explicitly retryable runner failures.
    pub async fn run_with_resource_class_and_timeout<F, Fut>(
        &self,
        task_id: impl Into<String>,
        class: ExecutionResourceClass,
        attempt_timeout: Option<Duration>,
        cancellation: watch::Receiver<bool>,
        mut operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, String>> + Send,
    {
        self.run_inner(
            task_id,
            class,
            attempt_timeout,
            cancellation,
            move |attempt, cancellation| {
                let operation_future = operation(attempt, cancellation);
                async move { operation_future.await.map_err(RunnerError::permanent) }
            },
        )
        .await
    }

    /// Run an operation that explicitly classifies failures as retryable or permanent.
    /// Unclassified string errors passed to run are always permanent.
    pub async fn run_with_retryable_errors<F, Fut>(
        &self,
        task_id: impl Into<String>,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, RunnerError>> + Send,
    {
        self.run_with_resource_class_and_retryable_errors(
            task_id,
            ExecutionResourceClass::General,
            cancellation,
            operation,
        )
        .await
    }

    /// Resource-class variant for operations with explicitly retryable errors.
    pub async fn run_with_resource_class_and_retryable_errors<F, Fut>(
        &self,
        task_id: impl Into<String>,
        class: ExecutionResourceClass,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, RunnerError>> + Send,
    {
        self.run_inner(task_id, class, None, cancellation, operation)
            .await
    }

    async fn run_inner<F, Fut>(
        &self,
        task_id: impl Into<String>,
        class: ExecutionResourceClass,
        attempt_timeout_override: Option<Duration>,
        mut cancellation: watch::Receiver<bool>,
        mut operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, RunnerError>> + Send,
    {
        let task_id = task_id.into();
        if task_id.trim().is_empty() {
            return Err(ExecutionError::InvalidTaskId);
        }
        let attempt_timeout = attempt_timeout_override.unwrap_or(self.config.attempt_timeout);
        if attempt_timeout.is_zero() {
            return Err(ExecutionError::InvalidTimeout);
        }
        let _permit = self.acquire_resource(class, &mut cancellation).await?;
        let started = Instant::now();
        let mut attempts = 0;
        while attempts < self.config.max_attempts {
            attempts += 1;
            let operation_future = operation(attempts, cancellation.clone());
            tokio::pin!(operation_future);
            let attempt_result = loop {
                tokio::select! {
                    result = tokio::time::timeout(attempt_timeout, &mut operation_future) => {
                        let cancellation_requested = *cancellation.borrow()
                            || matches!(cancellation.has_changed(), Ok(true) | Err(_));
                        if cancellation_requested {
                            return Err(ExecutionError::Cancelled { attempts });
                        }
                        break result.ok();
                    }
                    changed = cancellation.changed() => {
                        if changed.is_err() || *cancellation.borrow() {
                            // Give cooperative runners a bounded window to cancel
                            // provider-side work and release their resources.
                            let _ = tokio::time::timeout(
                                CANCELLATION_CLEANUP_GRACE,
                                &mut operation_future,
                            )
                            .await;
                            return Err(ExecutionError::Cancelled { attempts });
                        }
                    }
                }
            };

            match attempt_result {
                Some(Ok(result)) => {
                    return Ok(ExecutionResult {
                        task_id,
                        attempts,
                        result,
                        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                    });
                }
                Some(Err(error)) if error.retryable && attempts < self.config.max_attempts => {
                    self.wait_before_retry(attempts, &mut cancellation).await?;
                    continue;
                }
                Some(Err(error)) => {
                    return Err(ExecutionError::RunnerFailed {
                        attempts,
                        message: error.message,
                    });
                }
                // A timeout has an unknown side-effect outcome. Never replay it automatically.
                None => return Err(ExecutionError::TimedOut { attempts }),
            }
        }

        Err(ExecutionError::SchedulerClosed)
    }
}

async fn acquire_cancellable(
    semaphore: Arc<Semaphore>,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<tokio::sync::OwnedSemaphorePermit, ExecutionError> {
    loop {
        if *cancellation.borrow() {
            return Err(ExecutionError::Cancelled { attempts: 0 });
        }
        tokio::select! {
            permit = Arc::clone(&semaphore).acquire_owned() => {
                return permit.map_err(|_| ExecutionError::SchedulerClosed);
            }
            changed = cancellation.changed() => {
                if changed.is_err() || *cancellation.borrow() {
                    return Err(ExecutionError::Cancelled { attempts: 0 });
                }
            }
        }
    }
}

/// Per-node override for the maximum duration of one reviewed runner attempt.
/// Automatic retries are intentionally not user-configurable because they require
/// runner-specific idempotency and explicit retryable error classification.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl MissionExecutionPolicy {
    fn is_default(policy: &Self) -> bool {
        policy.timeout_ms.is_none()
    }
}

/// A dependency-aware mission plan. This model validates and orders work only;
/// execution and external side effects remain the responsibility of reviewed runners.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MissionNode {
    pub id: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Higher values are scheduled first when multiple nodes are ready.
    #[serde(default)]
    pub priority: u8,
    pub task_type: String,
    /// JSON payload passed to the reviewed task runner after operator approval.
    #[serde(default)]
    pub payload: Value,
    /// Bounded node-level execution controls; absent values inherit workspace defaults.
    #[serde(default, skip_serializing_if = "MissionExecutionPolicy::is_default")]
    pub execution_policy: MissionExecutionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MissionGraph {
    pub mission_id: String,
    pub nodes: Vec<MissionNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionGraphError {
    EmptyMissionId,
    EmptyNodeId,
    DuplicateNodeId(String),
    EmptyTaskType(String),
    MissingDependency { node: String, dependency: String },
    SelfDependency(String),
    DependencyCycle(Vec<String>),
}

impl MissionGraph {
    /// Validate dependencies and return a deterministic priority-aware execution order.
    pub fn execution_order(&self) -> Result<Vec<String>, MissionGraphError> {
        use std::cmp::Reverse;
        use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

        if self.mission_id.trim().is_empty() {
            return Err(MissionGraphError::EmptyMissionId);
        }

        let mut nodes = BTreeMap::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() {
                return Err(MissionGraphError::EmptyNodeId);
            }
            if node.task_type.trim().is_empty() {
                return Err(MissionGraphError::EmptyTaskType(node.id.clone()));
            }
            if nodes.insert(node.id.as_str(), node).is_some() {
                return Err(MissionGraphError::DuplicateNodeId(node.id.clone()));
            }
        }

        let mut indegree = BTreeMap::<&str, usize>::new();
        let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
        for node in &self.nodes {
            let mut unique = BTreeSet::new();
            for dependency in &node.depends_on {
                if dependency == &node.id {
                    return Err(MissionGraphError::SelfDependency(node.id.clone()));
                }
                if !nodes.contains_key(dependency.as_str()) {
                    return Err(MissionGraphError::MissingDependency {
                        node: node.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
                if unique.insert(dependency.as_str()) {
                    *indegree.entry(node.id.as_str()).or_default() += 1;
                    dependents
                        .entry(dependency.as_str())
                        .or_default()
                        .push(node.id.as_str());
                }
            }
            indegree.entry(node.id.as_str()).or_default();
        }

        // Reverse ordering makes BinaryHeap pop highest priority, then lexical ID.
        let mut ready = BinaryHeap::new();
        for node in &self.nodes {
            if indegree[node.id.as_str()] == 0 {
                ready.push((node.priority, Reverse(node.id.as_str())));
            }
        }

        let mut ordered = Vec::with_capacity(self.nodes.len());
        while let Some((_priority, Reverse(id))) = ready.pop() {
            ordered.push(id.to_string());
            if let Some(children) = dependents.get(id) {
                for child in children {
                    let remaining = indegree.get_mut(child).expect("known dependent");
                    *remaining -= 1;
                    if *remaining == 0 {
                        let node = nodes[child];
                        ready.push((node.priority, Reverse(node.id.as_str())));
                    }
                }
            }
        }

        if ordered.len() != self.nodes.len() {
            let mut cycle_nodes = indegree
                .into_iter()
                .filter(|(_, degree)| *degree > 0)
                .map(|(id, _)| id.to_string())
                .collect::<Vec<_>>();
            cycle_nodes.sort();
            return Err(MissionGraphError::DependencyCycle(cycle_nodes));
        }
        Ok(ordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{Notify, oneshot};

    #[test]
    fn mission_graph_orders_dependencies_before_priority() {
        let graph = MissionGraph {
            mission_id: "mission-1".into(),
            nodes: vec![
                MissionNode {
                    id: "report".into(),
                    depends_on: vec!["build".into()],
                    priority: 250,
                    task_type: "report".into(),
                    payload: Value::Null,
                    execution_policy: MissionExecutionPolicy::default(),
                },
                MissionNode {
                    id: "audit".into(),
                    depends_on: vec![],
                    priority: 20,
                    task_type: "audit".into(),
                    payload: Value::Null,
                    execution_policy: MissionExecutionPolicy::default(),
                },
                MissionNode {
                    id: "build".into(),
                    depends_on: vec![],
                    priority: 1,
                    task_type: "build".into(),
                    payload: Value::Null,
                    execution_policy: MissionExecutionPolicy::default(),
                },
            ],
        };
        assert_eq!(
            graph.execution_order().unwrap(),
            vec!["audit", "build", "report"]
        );
    }

    #[test]
    fn mission_graph_rejects_missing_dependencies_and_cycles() {
        let missing = MissionGraph {
            mission_id: "m".into(),
            nodes: vec![MissionNode {
                id: "a".into(),
                depends_on: vec!["absent".into()],
                priority: 0,
                task_type: "x".into(),
                payload: Value::Null,
                execution_policy: MissionExecutionPolicy::default(),
            }],
        };
        assert_eq!(
            missing.execution_order(),
            Err(MissionGraphError::MissingDependency {
                node: "a".into(),
                dependency: "absent".into()
            })
        );

        let cycle = MissionGraph {
            mission_id: "m".into(),
            nodes: vec![
                MissionNode {
                    id: "a".into(),
                    depends_on: vec!["b".into()],
                    priority: 0,
                    task_type: "x".into(),
                    payload: Value::Null,
                    execution_policy: MissionExecutionPolicy::default(),
                },
                MissionNode {
                    id: "b".into(),
                    depends_on: vec!["a".into()],
                    priority: 0,
                    task_type: "x".into(),
                    payload: Value::Null,
                },
            ],
        };
        assert_eq!(
            cycle.execution_order(),
            Err(MissionGraphError::DependencyCycle(vec![
                "a".into(),
                "b".into()
            ]))
        );
    }

    fn config(max_concurrency: usize, max_admitted: usize) -> ExecutionConfig {
        ExecutionConfig {
            max_concurrency,
            max_admitted,
            max_attempts: 1,
            attempt_timeout: Duration::from_secs(2),
            retry_backoff: Duration::ZERO,
            max_retry_backoff: Duration::ZERO,
        }
    }

    #[test]
    fn rejects_zero_resource_class_capacity() {
        let limits = ResourceLimits {
            agent: 0,
            ..ResourceLimits::default()
        };
        assert_eq!(
            ExecutionGate::new_with_resource_limits(config(2, 4), limits).err(),
            Some(ConfigError::ZeroResourceConcurrency {
                class: ExecutionResourceClass::Agent
            })
        );
    }

    #[tokio::test]
    async fn enforces_agent_class_limit_independently_of_global_capacity() {
        let limits = ResourceLimits {
            agent: 1,
            ..ResourceLimits::default()
        };
        let gate = Arc::new(ExecutionGate::new_with_resource_limits(config(2, 4), limits).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));

        let spawn_agent = |name: &'static str| {
            let gate = Arc::clone(&gate);
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            tokio::spawn(async move {
                let (_cancel, receiver) = watch::channel(false);
                gate.run_with_resource_class(
                    name,
                    ExecutionResourceClass::Agent,
                    receiver,
                    move |_, _| {
                        let active = Arc::clone(&active);
                        let maximum = Arc::clone(&maximum);
                        async move {
                            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                            maximum.fetch_max(now, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(25)).await;
                            active.fetch_sub(1, Ordering::SeqCst);
                            Ok(json!({"done": true}))
                        }
                    },
                )
                .await
            })
        };

        let first = spawn_agent("agent-one");
        let second = spawn_agent("agent-two");
        assert!(first.await.unwrap().is_ok());
        assert!(second.await.unwrap().is_ok());
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn different_resource_classes_share_global_but_not_class_limits() {
        let limits = ResourceLimits {
            agent: 1,
            network: 1,
            ..ResourceLimits::default()
        };
        let gate = Arc::new(ExecutionGate::new_with_resource_limits(config(2, 4), limits).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));

        let spawn = |name: &'static str, class: ExecutionResourceClass| {
            let gate = Arc::clone(&gate);
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            tokio::spawn(async move {
                let (_cancel, receiver) = watch::channel(false);
                gate.run_with_resource_class(name, class, receiver, move |_, _| {
                    let active = Arc::clone(&active);
                    let maximum = Arc::clone(&maximum);
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        maximum.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(json!({"done": true}))
                    }
                })
                .await
            })
        };

        let agent = spawn("agent", ExecutionResourceClass::Agent);
        let network = spawn("network", ExecutionResourceClass::Network);
        assert!(agent.await.unwrap().is_ok());
        assert!(network.await.unwrap().is_ok());
        assert_eq!(maximum.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancellation_while_waiting_for_resource_class_does_not_start_runner() {
        let limits = ResourceLimits {
            agent: 1,
            ..ResourceLimits::default()
        };
        let gate = Arc::new(ExecutionGate::new_with_resource_limits(config(2, 4), limits).unwrap());
        let (cancel_first, receiver_first) = watch::channel(false);
        let (cancel_second, receiver_second) = watch::channel(false);
        let started = Arc::new(AtomicUsize::new(0));

        let first_gate = Arc::clone(&gate);
        let first_started = Arc::clone(&started);
        let first = tokio::spawn(async move {
            first_gate
                .run_with_resource_class(
                    "holding-agent",
                    ExecutionResourceClass::Agent,
                    receiver_first,
                    move |_, _| {
                        let started = Arc::clone(&first_started);
                        async move {
                            started.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            Ok(Value::Null)
                        }
                    },
                )
                .await
        });

        for _ in 0..50 {
            if started.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let second_gate = Arc::clone(&gate);
        let second_started = Arc::clone(&started);
        let second = tokio::spawn(async move {
            second_gate
                .run_with_resource_class(
                    "cancelled-agent",
                    ExecutionResourceClass::Agent,
                    receiver_second,
                    move |_, _| {
                        let started = Arc::clone(&second_started);
                        async move {
                            started.fetch_add(1, Ordering::SeqCst);
                            Ok(Value::Null)
                        }
                    },
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        cancel_second.send(true).unwrap();
        assert_eq!(
            second.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 0 })
        );
        assert_eq!(started.load(Ordering::SeqCst), 1);
        cancel_first.send(true).unwrap();
        assert_eq!(
            first.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 1 })
        );
    }

    #[test]
    fn mission_execution_policy_is_backward_compatible_and_round_trips() {
        let legacy = serde_json::from_value::<MissionNode>(json!({
            "id": "node",
            "task_type": "service_discovery"
        }))
        .unwrap();
        assert_eq!(legacy.execution_policy, MissionExecutionPolicy::default());

        let configured = serde_json::from_value::<MissionNode>(json!({
            "id": "node",
            "task_type": "gpu_chat",
            "execution_policy": {"timeout_ms": 15000}
        }))
        .unwrap();
        assert_eq!(configured.execution_policy.timeout_ms, Some(15000));
        let encoded = serde_json::to_value(configured).unwrap();
        assert_eq!(encoded["execution_policy"]["timeout_ms"], 15000);
    }

    #[tokio::test]
    async fn per_node_timeout_override_is_enforced_without_replaying_work() {
        let gate = ExecutionGate::new(config(1, 2)).unwrap();
        let (_cancel, receiver) = watch::channel(false);
        let result = gate
            .run_with_resource_class_and_timeout(
                "mission-node-timeout",
                ExecutionResourceClass::GpuInference,
                Some(Duration::from_millis(10)),
                receiver,
                |_, _| async {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(Value::Null)
                },
            )
            .await;
        assert_eq!(result, Err(ExecutionError::TimedOut { attempts: 1 }));
        assert_eq!(gate.admitted_count(), 0);
    }

    #[tokio::test]
    async fn rejects_zero_per_node_timeout_before_admission() {
        let gate = ExecutionGate::new(config(1, 2)).unwrap();
        let (_cancel, receiver) = watch::channel(false);
        let result = gate
            .run_with_resource_class_and_timeout(
                "zero-timeout",
                ExecutionResourceClass::General,
                Some(Duration::ZERO),
                receiver,
                |_, _| async { Ok(Value::Null) },
            )
            .await;
        assert_eq!(result, Err(ExecutionError::InvalidTimeout));
        assert_eq!(gate.admitted_count(), 0);
    }

    #[test]
    fn default_timeout_covers_supervisor_bridge_inference_window() {
        assert_eq!(
            ExecutionConfig::default().attempt_timeout,
            Duration::from_secs(1805)
        );
    }

    #[test]
    fn rejects_invalid_limits() {
        assert_eq!(
            ExecutionGate::new(config(0, 1)).err(),
            Some(ConfigError::ZeroConcurrency)
        );
        assert_eq!(
            ExecutionGate::new(config(2, 1)).err(),
            Some(ConfigError::AdmissionBelowConcurrency)
        );
        let mut invalid_backoff = config(1, 1);
        invalid_backoff.retry_backoff = Duration::from_secs(2);
        invalid_backoff.max_retry_backoff = Duration::from_secs(1);
        assert_eq!(
            ExecutionGate::new(invalid_backoff).err(),
            Some(ConfigError::MaxRetryBackoffBelowInitial)
        );
    }

    #[tokio::test]
    async fn enforces_execution_concurrency_limit() {
        let gate = Arc::new(ExecutionGate::new(config(1, 2)).unwrap());
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let (_cancel_a, rx_a) = watch::channel(false);
        let (_cancel_b, rx_b) = watch::channel(false);
        let first_gate = Arc::clone(&gate);
        let first_active = Arc::clone(&active);
        let first_maximum = Arc::clone(&maximum);
        let second_gate = Arc::clone(&gate);
        let second_active = Arc::clone(&active);
        let second_maximum = Arc::clone(&maximum);

        let first = tokio::spawn(async move {
            first_gate
                .run("first", rx_a, move |_, _| {
                    let active = Arc::clone(&first_active);
                    let maximum = Arc::clone(&first_maximum);
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        maximum.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(json!({"done": true}))
                    }
                })
                .await
        });
        let second = tokio::spawn(async move {
            second_gate
                .run("second", rx_b, move |_, _| {
                    let active = Arc::clone(&second_active);
                    let maximum = Arc::clone(&second_maximum);
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        maximum.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(json!({"done": true}))
                    }
                })
                .await
        });

        assert!(first.await.unwrap().is_ok());
        assert!(second.await.unwrap().is_ok());
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_work_when_admission_limit_is_full() {
        let gate = Arc::new(ExecutionGate::new(config(1, 1)).unwrap());
        let release = Arc::new(Notify::new());
        let (started_tx, started_rx) = oneshot::channel();
        let (_cancel_first, rx_first) = watch::channel(false);
        let first_gate = Arc::clone(&gate);
        let first_release = Arc::clone(&release);
        let first = tokio::spawn(async move {
            let mut started_tx = Some(started_tx);
            first_gate
                .run("held", rx_first, move |_, _| {
                    let release = Arc::clone(&first_release);
                    let started = started_tx.take();
                    async move {
                        if let Some(started) = started {
                            let _ = started.send(());
                        }
                        release.notified().await;
                        Ok(json!({"released": true}))
                    }
                })
                .await
        });
        started_rx.await.unwrap();

        let (_cancel_second, rx_second) = watch::channel(false);
        let second = gate
            .run("overflow", rx_second, |_, _| async {
                Ok(json!({"unexpected": true}))
            })
            .await;
        assert_eq!(second, Err(ExecutionError::AdmissionFull { limit: 1 }));

        release.notify_one();
        assert!(first.await.unwrap().is_ok());
        assert_eq!(gate.admitted_count(), 0);
    }

    #[tokio::test]
    async fn retries_runner_failures_only_up_to_configured_limit() {
        let mut retry_config = config(1, 1);
        retry_config.max_attempts = 2;
        let gate = ExecutionGate::new(retry_config).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_runner = Arc::clone(&calls);
        let (_cancel, receiver) = watch::channel(false);

        let result = gate
            .run_with_retryable_errors("retry", receiver, move |_, _| {
                let calls = Arc::clone(&calls_for_runner);
                async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        Err(RunnerError::retryable("temporary transport failure"))
                    } else {
                        Ok(json!({"ok": true}))
                    }
                }
            })
            .await
            .unwrap();

        assert_eq!(result.attempts, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_wait_is_cancellable_before_next_attempt() {
        let mut retry_config = config(1, 1);
        retry_config.max_attempts = 3;
        retry_config.retry_backoff = Duration::from_secs(5);
        retry_config.max_retry_backoff = Duration::from_secs(10);
        let gate = ExecutionGate::new(retry_config).unwrap();
        let (cancel, receiver) = watch::channel(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_runner = Arc::clone(&calls);

        let task = tokio::spawn(async move {
            gate.run_with_retryable_errors("cancel-during-backoff", receiver, move |_, _| {
                let calls = Arc::clone(&calls_for_runner);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(RunnerError::retryable("temporary transport failure"))
                }
            })
            .await
        });
        for _ in 0..50 {
            if calls.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        cancel.send(true).unwrap();
        assert_eq!(
            task.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 1 })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn permanent_runner_errors_are_not_retried() {
        let mut retry_config = config(1, 1);
        retry_config.max_attempts = 3;
        let gate = ExecutionGate::new(retry_config).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_runner = Arc::clone(&calls);
        let (_cancel, receiver) = watch::channel(false);

        let result = gate
            .run("permanent-error", receiver, move |_, _| {
                let calls = Arc::clone(&calls_for_runner);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err("invalid task input".to_string())
                }
            })
            .await;

        assert_eq!(
            result,
            Err(ExecutionError::RunnerFailed {
                attempts: 1,
                message: "invalid task input".into()
            })
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_stops_an_active_attempt() {
        let gate = Arc::new(ExecutionGate::new(config(1, 1)).unwrap());
        let (cancel, receiver) = watch::channel(false);
        let task = tokio::spawn(async move {
            gate.run("cancel-me", receiver, |_, _| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(Value::Null)
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.send(true).unwrap();
        assert_eq!(
            task.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 1 })
        );
    }

    #[tokio::test]
    async fn cancellation_gives_active_runner_a_bounded_cleanup_window() {
        let gate = Arc::new(ExecutionGate::new(config(1, 1)).unwrap());
        let (cancel, receiver) = watch::channel(false);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let mut started_tx = Some(started_tx);
        let cleanup = Arc::new(AtomicUsize::new(0));
        let cleanup_for_runner = Arc::clone(&cleanup);
        let task = tokio::spawn(async move {
            gate.run("cancel-cleanup", receiver, move |_, mut cancellation| {
                let cleanup = Arc::clone(&cleanup_for_runner);
                let started_tx = started_tx.take();
                async move {
                    if let Some(started_tx) = started_tx {
                        let _ = started_tx.send(());
                    }
                    let _ = cancellation.changed().await;
                    cleanup.fetch_add(1, Ordering::SeqCst);
                    Err("provider cancellation completed".to_string())
                }
            })
            .await
        });
        started_rx.await.unwrap();
        cancel.send(true).unwrap();

        assert_eq!(
            task.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 1 })
        );
        assert_eq!(cleanup.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn times_out_an_attempt_without_exceeding_retry_limit() {
        let mut timeout_config = config(1, 1);
        timeout_config.max_attempts = 3;
        timeout_config.attempt_timeout = Duration::from_millis(10);
        let gate = ExecutionGate::new(timeout_config).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_runner = Arc::clone(&calls);
        let (_cancel, receiver) = watch::channel(false);
        let result = gate
            .run("timeout", receiver, move |_, _| {
                let calls = Arc::clone(&calls_for_runner);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok(Value::Null)
                }
            })
            .await;
        assert_eq!(result, Err(ExecutionError::TimedOut { attempts: 1 }));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
