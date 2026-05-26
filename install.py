#!/usr/bin/env python3
"""Install dependencies and make hyperagent available"""
import subprocess, sys

deps = ['requests', 'rich']
for dep in deps:
    subprocess.check_call([sys.executable, '-m', 'pip', 'install', dep, '-q'])
print("✅ Dependencies installed")

# Create symlink
import os
script_path = os.path.abspath(__file__)
hyper_path = os.path.join(os.path.dirname(script_path), 'hyper.py')
bin_dir = '/usr/local/bin'
hyper_bin = os.path.join(bin_dir, 'hyper')

if os.path.exists(hyper_bin):
    os.remove(hyper_bin)
os.symlink(hyper_path, hyper_bin)
os.chmod(hyper_path, 0o755)
print(f"✅ Installed: {hyper_bin}")
print("   Run: hyper --init  (in your project directory)")
