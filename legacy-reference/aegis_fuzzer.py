import ast
import sys
import subprocess
import tempfile
import os

print("=" * 65)
print("  PROJECT DREAD: AEGIS AST SECURITY FUZZER INITIALIZING")
print("=" * 65)

class AegisASTVisitor(ast.NodeVisitor):
    def __init__(self):
        self.vulnerabilities = []

    def visit_Call(self, node):
        if isinstance(node.func, ast.Name):
            if node.func.id in ['eval', 'exec', 'input']:
                self.vulnerabilities.append(f"[!] Dangerous built-in call '{node.func.id}' detected at line {node.lineno}")
        elif isinstance(node.func, ast.Attribute):
            if node.func.attr in ['system', 'popen', 'loads'] and isinstance(node.func.value, ast.Name):
                self.vulnerabilities.append(f"[!] Dangerous module call '{node.func.value.id}.{node.func.attr}' detected at line {node.lineno}")
        self.generic_visit(node)

# Test target snippet containing intentional vulnerability patterns
test_code = """
import os
import pickle

def unsafe_handler(user_input):
    eval(user_input)
    os.system("echo 'running arbitrary command'")
    data = pickle.loads(user_input)
"""

print("[+] Parsing Target AST Structure...")
tree = ast.parse(test_code)
visitor = AegisASTVisitor()
visitor.visit(tree)

if visitor.vulnerabilities:
    print("[+] AST Vulnerability Scan Results:")
    for vuln in visitor.vulnerabilities:
        print(f"    {vuln}")
else:
    print("    [+] No vulnerabilities detected.")

print("[+] Aegis AST Fuzzer Ready.")
print("=" * 65)
