//! Bounded execution primitives for DREAD.
//!
//! This crate does not discover providers, execute plugins, or own GPU resources.
//! Callers supply an explicitly selected runner. The existing Python GPU Supervisor
//! remains the sole authority for GPU admission and lifecycle.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

const CANCELLATION_CLEANUP_GRACE: Duration = Duration::from_secs(5);

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

/// Independently bounded resource dimensions for trusted scheduler admission.
///
/// These quotas limit DREAD's own worker concurrency. They do not replace provider
/// admission or hardware ownership; the Python GPU Supervisor remains authoritative
/// for actual GPU process and lease lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClass {
    Cpu,
    Gpu,
    Model,
    Media,
    Agent,
}

impl ResourceClass {
    const ALL: [Self; 5] = [Self::Cpu, Self::Gpu, Self::Model, Self::Media, Self::Agent];

    const fn bit(self) -> u8 {
        match self {
            Self::Cpu => 0b00001,
            Self::Gpu => 0b00010,
            Self::Model => 0b00100,
            Self::Media => 0b01000,
            Self::Agent => 0b10000,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Cpu => 0,
            Self::Gpu => 1,
            Self::Model => 2,
            Self::Media => 3,
            Self::Agent => 4,
        }
    }
}

/// Resource dimensions selected by trusted runner routing, never by an API payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceRequest(u8);

impl ResourceRequest {
    pub const CPU: Self = Self(ResourceClass::Cpu.bit());
    pub const GPU: Self = Self(ResourceClass::Gpu.bit());
    pub const MODEL: Self = Self(ResourceClass::Model.bit());
    pub const MEDIA: Self = Self(ResourceClass::Media.bit());
    pub const AGENT: Self = Self(ResourceClass::Agent.bit());
    /// Local model inference requires both the GPU and model class budgets.
    pub const GPU_MODEL: Self = Self(ResourceClass::Gpu.bit() | ResourceClass::Model.bit());
    /// Reserved for reviewed media runners; this does not acquire a hardware lease.
    pub const GPU_MEDIA: Self = Self(ResourceClass::Gpu.bit() | ResourceClass::Media.bit());

    fn includes(self, class: ResourceClass) -> bool {
        self.0 & class.bit() != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Per-class execution ceilings. The global ExecutionConfig cap still applies across
/// all classes; tasks requesting multiple classes acquire each permit in stable order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub cpu: usize,
    pub gpu: usize,
    pub model: usize,
    pub media: usize,
    pub agent: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu: 2,
            gpu: 1,
            model: 1,
            media: 1,
            agent: 1,
        }
    }
}

impl ResourceLimits {
    const fn for_class(self, class: ResourceClass) -> usize {
        match class {
            ResourceClass::Cpu => self.cpu,
            ResourceClass::Gpu => self.gpu,
            ResourceClass::Model => self.model,
            ResourceClass::Media => self.media,
            ResourceClass::Agent => self.agent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    ZeroConcurrency,
    AdmissionBelowConcurrency,
    ZeroResourceConcurrency(ResourceClass),
    ZeroAttempts,
    ZeroTimeout,
    MaxRetryBackoffBelowInitial,
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
    InvalidResourceRequest,
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
    resource_semaphores: [Arc<Semaphore>; 5],
    admitted: Arc<AtomicUsize>,
    config: ExecutionConfig,
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
        resource_limits: ResourceLimits,
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
        for class in ResourceClass::ALL {
            if resource_limits.for_class(class) == 0 {
                return Err(ConfigError::ZeroResourceConcurrency(class));
            }
        }
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrency)),
            resource_semaphores: std::array::from_fn(|index| {
                Arc::new(Semaphore::new(
                    resource_limits.for_class(ResourceClass::ALL[index]),
                ))
            }),
            admitted: Arc::new(AtomicUsize::new(0)),
            config,
        })
    }

    pub fn admitted_count(&self) -> usize {
        self.admitted.load(Ordering::Acquire)
    }

    pub fn available_resource_permits(&self, class: ResourceClass) -> usize {
        self.resource_semaphores[class.index()].available_permits()
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

    /// Acquire each requested resource in enum order, then the shared global
    /// concurrency permit. Waiting for a busy resource class does not consume a
    /// global running slot, and cancellation drops every permit already acquired.
    async fn acquire_permits(
        &self,
        request: ResourceRequest,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<(Vec<OwnedSemaphorePermit>, OwnedSemaphorePermit), ExecutionError> {
        if request.is_empty() {
            return Err(ExecutionError::InvalidResourceRequest);
        }

        let mut resources = Vec::new();
        for class in ResourceClass::ALL {
            if !request.includes(class) {
                continue;
            }
            let permit = loop {
                if *cancellation.borrow() {
                    return Err(ExecutionError::Cancelled { attempts: 0 });
                }
                tokio::select! {
                    permit = Arc::clone(&self.resource_semaphores[class.index()]).acquire_owned() => {
                        break permit.map_err(|_| ExecutionError::SchedulerClosed)?;
                    }
                    changed = cancellation.changed() => {
                        if changed.is_err() || *cancellation.borrow() {
                            return Err(ExecutionError::Cancelled { attempts: 0 });
                        }
                    }
                }
            };
            resources.push(permit);
        }

        let global = loop {
            if *cancellation.borrow() {
                return Err(ExecutionError::Cancelled { attempts: 0 });
            }
            tokio::select! {
                permit = Arc::clone(&self.semaphore).acquire_owned() => {
                    break permit.map_err(|_| ExecutionError::SchedulerClosed)?;
                }
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        return Err(ExecutionError::Cancelled { attempts: 0 });
                    }
                }
            }
        };
        Ok((resources, global))
    }

    /// Run a long-lived managed operation under the shared admission and concurrency limits.
    ///
    /// This is intended for provider workers whose own lifecycle polls for terminal
    /// state and performs provider-specific cancellation. It deliberately does not
    /// apply the generic attempt timeout or automatic retry: dropping a remote
    /// request can leave non-idempotent provider work running after DREAD has lost
    /// track of it. The supplied operation must observe cancellation and return only
    /// after it has confirmed or completed provider-side cleanup.
    pub async fn run_managed<T, F, Fut>(
        &self,
        task_id: impl Into<String>,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<T, ExecutionError>
    where
        T: Send,
        F: FnOnce(watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
    {
        self.run_managed_for_resources(ResourceRequest::AGENT, task_id, cancellation, operation)
            .await
    }

    /// Run a long-lived provider operation under trusted resource-class and global limits.
    /// The runner must observe cancellation and finish provider cleanup before returning.
    pub async fn run_managed_for_resources<T, F, Fut>(
        &self,
        resources: ResourceRequest,
        task_id: impl Into<String>,
        mut cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<T, ExecutionError>
    where
        T: Send,
        F: FnOnce(watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<T, String>> + Send,
    {
        let task_id = task_id.into();
        if task_id.trim().is_empty() {
            return Err(ExecutionError::InvalidTaskId);
        }

        let _admission = self.try_admit()?;
        let (_resource_permits, _global_permit) =
            self.acquire_permits(resources, &mut cancellation).await?;
        operation(cancellation.clone())
            .await
            .map_err(|message| ExecutionError::RunnerFailed {
                attempts: 1,
                message,
            })
    }

    /// Run a caller-supplied operation under bounded admission and concurrency.
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
        self.run_for_resources(ResourceRequest::CPU, task_id, cancellation, operation)
            .await
    }

    /// Run a reviewed operation using only the resource classes chosen by trusted code.
    pub async fn run_for_resources<F, Fut>(
        &self,
        resources: ResourceRequest,
        task_id: impl Into<String>,
        cancellation: watch::Receiver<bool>,
        mut operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, String>> + Send,
    {
        self.run_inner(
            resources,
            task_id,
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
        self.run_with_retryable_errors_for_resources(
            ResourceRequest::CPU,
            task_id,
            cancellation,
            operation,
        )
        .await
    }

    pub async fn run_with_retryable_errors_for_resources<F, Fut>(
        &self,
        resources: ResourceRequest,
        task_id: impl Into<String>,
        cancellation: watch::Receiver<bool>,
        operation: F,
    ) -> Result<ExecutionResult, ExecutionError>
    where
        F: FnMut(u8, watch::Receiver<bool>) -> Fut + Send,
        Fut: Future<Output = Result<Value, RunnerError>> + Send,
    {
        self.run_inner(resources, task_id, cancellation, operation)
            .await
    }

    async fn run_inner<F, Fut>(
        &self,
        resources: ResourceRequest,
        task_id: impl Into<String>,
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
        let _admission = self.try_admit()?;
        let (_resource_permits, _global_permit) =
            self.acquire_permits(resources, &mut cancellation).await?;

        let started = Instant::now();
        let mut attempts = 0;
        while attempts < self.config.max_attempts {
            attempts += 1;
            let operation_future = operation(attempts, cancellation.clone());
            tokio::pin!(operation_future);
            let attempt_result = loop {
                tokio::select! {
                    result = tokio::time::timeout(self.config.attempt_timeout, &mut operation_future) => {
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
                },
                MissionNode {
                    id: "audit".into(),
                    depends_on: vec![],
                    priority: 20,
                    task_type: "audit".into(),
                    payload: Value::Null,
                },
                MissionNode {
                    id: "build".into(),
                    depends_on: vec![],
                    priority: 1,
                    task_type: "build".into(),
                    payload: Value::Null,
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
    async fn managed_runner_shares_admission_and_concurrency_with_regular_runners() {
        let gate = Arc::new(ExecutionGate::new(config(1, 2)).unwrap());
        let release = Arc::new(Notify::new());
        let (managed_started_tx, managed_started_rx) = oneshot::channel();
        let managed_gate = Arc::clone(&gate);
        let managed_release = Arc::clone(&release);

        let managed = tokio::spawn(async move {
            managed_gate
                .run_managed("agent-task", watch::channel(false).1, move |_| async move {
                    let _ = managed_started_tx.send(());
                    managed_release.notified().await;
                    Ok(json!({"managed": true}))
                })
                .await
        });
        managed_started_rx.await.unwrap();

        let (regular_started_tx, mut regular_started_rx) = oneshot::channel();
        let regular_gate = Arc::clone(&gate);
        let regular = tokio::spawn(async move {
            let mut started_tx = Some(regular_started_tx);
            regular_gate
                .run("builtin-task", watch::channel(false).1, move |_, _| {
                    let started = started_tx.take();
                    async move {
                        if let Some(started) = started {
                            let _ = started.send(());
                        }
                        Ok(json!({"regular": true}))
                    }
                })
                .await
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut regular_started_rx)
                .await
                .is_err(),
            "the regular task must wait while the managed provider task owns the shared permit"
        );

        release.notify_one();
        assert!(managed.await.unwrap().is_ok());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut regular_started_rx)
                .await
                .unwrap()
                .is_ok()
        );
        assert!(regular.await.unwrap().is_ok());
        assert_eq!(gate.admitted_count(), 0);
    }

    #[test]
    fn rejects_zero_resource_class_limit() {
        let limits = ResourceLimits {
            media: 0,
            ..ResourceLimits::default()
        };
        assert_eq!(
            ExecutionGate::new_with_resource_limits(config(2, 4), limits).err(),
            Some(ConfigError::ZeroResourceConcurrency(ResourceClass::Media))
        );
    }

    #[tokio::test]
    async fn resource_quotas_do_not_occupy_global_slots_while_waiting() {
        let limits = ResourceLimits {
            cpu: 2,
            gpu: 1,
            model: 1,
            media: 1,
            agent: 1,
        };
        let gate = Arc::new(ExecutionGate::new_with_resource_limits(config(2, 6), limits).unwrap());
        let release_gpu = Arc::new(Notify::new());

        let (first_started_tx, first_started_rx) = oneshot::channel();
        let first_gate = Arc::clone(&gate);
        let first_release = Arc::clone(&release_gpu);
        let first = tokio::spawn(async move {
            let mut started = Some(first_started_tx);
            first_gate
                .run_for_resources(
                    ResourceRequest::GPU_MODEL,
                    "gpu-model-1",
                    watch::channel(false).1,
                    move |_, _| {
                        let started = started.take();
                        let release = Arc::clone(&first_release);
                        async move {
                            if let Some(started) = started {
                                let _ = started.send(());
                            }
                            release.notified().await;
                            Ok(json!({"gpu": 1}))
                        }
                    },
                )
                .await
        });
        first_started_rx.await.unwrap();

        let (second_started_tx, mut second_started_rx) = oneshot::channel();
        let second_gate = Arc::clone(&gate);
        let second = tokio::spawn(async move {
            let mut started = Some(second_started_tx);
            second_gate
                .run_for_resources(
                    ResourceRequest::GPU_MODEL,
                    "gpu-model-2",
                    watch::channel(false).1,
                    move |_, _| {
                        let started = started.take();
                        async move {
                            if let Some(started) = started {
                                let _ = started.send(());
                            }
                            Ok(json!({"gpu": 2}))
                        }
                    },
                )
                .await
        });

        for _ in 0..50 {
            if gate.admitted_count() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(gate.admitted_count(), 2);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut second_started_rx)
                .await
                .is_err(),
            "a second GPU/model task must wait behind the GPU and model class ceilings"
        );

        let (cpu_started_tx, cpu_started_rx) = oneshot::channel();
        let cpu_result = gate
            .run_for_resources(
                ResourceRequest::CPU,
                "cpu-discovery",
                watch::channel(false).1,
                {
                    let mut started = Some(cpu_started_tx);
                    move |_, _| {
                        let started = started.take();
                        async move {
                            if let Some(started) = started {
                                let _ = started.send(());
                            }
                            Ok(json!({"cpu": true}))
                        }
                    }
                },
            )
            .await;
        assert!(cpu_result.is_ok());
        cpu_started_rx.await.unwrap();

        release_gpu.notify_one();
        assert!(first.await.unwrap().is_ok());
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut second_started_rx)
                .await
                .unwrap()
                .is_ok()
        );
        assert!(second.await.unwrap().is_ok());
        assert_eq!(gate.admitted_count(), 0);
        assert_eq!(gate.available_resource_permits(ResourceClass::Gpu), 1);
        assert_eq!(gate.available_resource_permits(ResourceClass::Model), 1);
    }

    #[tokio::test]
    async fn cancellation_while_waiting_for_resource_releases_admission() {
        let gate = Arc::new(
            ExecutionGate::new_with_resource_limits(
                config(2, 4),
                ResourceLimits {
                    cpu: 2,
                    gpu: 1,
                    model: 1,
                    media: 1,
                    agent: 1,
                },
            )
            .unwrap(),
        );
        let release_gpu = Arc::new(Notify::new());
        let (started_tx, started_rx) = oneshot::channel();
        let first_gate = Arc::clone(&gate);
        let first_release = Arc::clone(&release_gpu);
        let first = tokio::spawn(async move {
            let mut started = Some(started_tx);
            first_gate
                .run_for_resources(
                    ResourceRequest::GPU_MODEL,
                    "gpu-holder",
                    watch::channel(false).1,
                    move |_, _| {
                        let started = started.take();
                        let release = Arc::clone(&first_release);
                        async move {
                            if let Some(started) = started {
                                let _ = started.send(());
                            }
                            release.notified().await;
                            Ok(json!({"held": true}))
                        }
                    },
                )
                .await
        });
        started_rx.await.unwrap();

        let (cancel, cancellation) = watch::channel(false);
        let second_gate = Arc::clone(&gate);
        let second = tokio::spawn(async move {
            second_gate
                .run_for_resources(
                    ResourceRequest::GPU_MODEL,
                    "gpu-waiter",
                    cancellation,
                    |_, _| async { Ok(json!({"should_not_run": true})) },
                )
                .await
        });
        for _ in 0..50 {
            if gate.admitted_count() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(gate.admitted_count(), 2);
        cancel.send(true).unwrap();
        assert_eq!(
            second.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 0 })
        );
        assert_eq!(gate.admitted_count(), 1);

        release_gpu.notify_one();
        assert!(first.await.unwrap().is_ok());
        assert_eq!(gate.admitted_count(), 0);

        let retry = gate
            .run_for_resources(
                ResourceRequest::GPU_MODEL,
                "gpu-after-cancel",
                watch::channel(false).1,
                |_, _| async { Ok(json!({"ok": true})) },
            )
            .await;
        assert!(retry.is_ok());
    }

    #[tokio::test]
    async fn resource_request_must_not_be_empty() {
        let gate = ExecutionGate::new(config(1, 2)).unwrap();
        let result = gate
            .run_for_resources(
                ResourceRequest(0),
                "empty-resource-set",
                watch::channel(false).1,
                |_, _| async { Ok(json!({"unexpected": true})) },
            )
            .await;
        assert_eq!(result, Err(ExecutionError::InvalidResourceRequest));
        assert_eq!(gate.admitted_count(), 0);
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

    #[tokio::test]
    async fn managed_runner_cancellation_releases_admission_while_waiting_for_permit() {
        let gate = Arc::new(ExecutionGate::new(config(1, 2)).unwrap());
        let release = Arc::new(Notify::new());
        let holder_started = Arc::new(Notify::new());
        let holder_gate = Arc::clone(&gate);
        let holder_started_for_runner = Arc::clone(&holder_started);
        let holder_release_for_runner = Arc::clone(&release);
        let holder = tokio::spawn(async move {
            holder_gate
                .run("holder", watch::channel(false).1, move |_, _| {
                    let started = Arc::clone(&holder_started_for_runner);
                    let release = Arc::clone(&holder_release_for_runner);
                    async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(json!({"holder": true}))
                    }
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), holder_started.notified())
            .await
            .expect("holder execution should start");

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let waiting_gate = Arc::clone(&gate);
        let waiting = tokio::spawn(async move {
            waiting_gate
                .run_managed("waiting-provider", cancel_rx, |_| async {
                    Ok(json!({"unexpected": "runner started"}))
                })
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while gate.admitted_count() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("managed task should be admitted while waiting for the shared permit");

        cancel_tx.send(true).unwrap();
        assert_eq!(
            waiting.await.unwrap(),
            Err(ExecutionError::Cancelled { attempts: 0 })
        );
        assert_eq!(gate.admitted_count(), 1);

        release.notify_one();
        assert!(holder.await.unwrap().is_ok());
        assert_eq!(gate.admitted_count(), 0);
    }

    #[tokio::test]
    async fn managed_runner_passes_cancellation_to_provider_cleanup() {
        let gate = Arc::new(ExecutionGate::new(config(1, 2)).unwrap());
        let cleaned_up = Arc::new(AtomicUsize::new(0));
        let cleanup_flag = Arc::clone(&cleaned_up);
        let (started_tx, started_rx) = oneshot::channel();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let managed = tokio::spawn(async move {
            gate.run_managed(
                "provider-cleanup",
                cancel_rx,
                move |mut cancellation| async move {
                    let _ = started_tx.send(());
                    cancellation
                        .changed()
                        .await
                        .map_err(|_| "cancellation channel closed unexpectedly".to_string())?;
                    if !*cancellation.borrow() {
                        return Err("provider cleanup received a non-cancellation update".into());
                    }
                    cleanup_flag.store(1, Ordering::Release);
                    Ok(json!({"provider_cleanup": "confirmed"}))
                },
            )
            .await
        });

        started_rx.await.unwrap();
        cancel_tx.send(true).unwrap();
        let result = managed.await.unwrap().unwrap();
        assert_eq!(result, json!({"provider_cleanup": "confirmed"}));
        assert_eq!(cleaned_up.load(Ordering::Acquire), 1);
    }
}
