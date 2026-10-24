import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from evaluate_correction import infer, run_inference


class OfflineProcessTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == 'linux', 'Linux seccomp integration')
    def test_network_denial_survives_exec_and_child_processes(self):
        check = 'import socket\ntry: socket.socket()\nexcept PermissionError: pass\nelse: raise RuntimeError("socket allowed")'
        code = 'import subprocess,sys\n' + check + '\nsubprocess.run([sys.executable,"-c",' + repr(check) + '],check=True)\nprint("denied")'
        wrapper = Path(__file__).resolve().parents[1]/'run-offline-linux.py'
        result = subprocess.run([sys.executable, str(wrapper), sys.executable, '-c', code],
                                capture_output=True, text=True, check=True, timeout=10)
        self.assertEqual(result.stdout.strip(), 'denied')

    @unittest.skipUnless(os.name == 'posix', 'POSIX process groups')
    def test_timeout_stops_native_descendants_of_a_wrapper(self):
        with tempfile.TemporaryDirectory() as directory:
            heartbeat = Path(directory)/'heartbeat'
            child = f'import time\nf=open({str(heartbeat)!r},"ab",buffering=0)\nwhile True: f.write(b"x"); time.sleep(0.01)'
            parent = 'import subprocess,sys,time\nsubprocess.Popen([sys.executable,"-c",' + repr(child) + '])\ntime.sleep(60)'
            with self.assertRaises(subprocess.TimeoutExpired):
                run_inference([sys.executable, '-c', parent], '', timeout=2)
            before = heartbeat.stat().st_size
            time.sleep(0.1)
            self.assertEqual(heartbeat.stat().st_size, before)

    def test_rss_converts_linux_kib_to_bytes_but_preserves_macos_bytes(self):
        result = subprocess.CompletedProcess([], 0, json.dumps(dict(text='ok', backend='cpu', offloaded_layers=0)),
                                             '123 maximum resident set size')
        for system, expected in [('Linux', 123*1024), ('Darwin', 123)]:
            with patch('evaluate_correction.platform.system', return_value=system), patch('evaluate_correction.run_inference', return_value=result):
                self.assertEqual(infer(Path('/binary'), Path('/model'), 'cpu', 'raw', '')['peak_rss_bytes'], expected)
