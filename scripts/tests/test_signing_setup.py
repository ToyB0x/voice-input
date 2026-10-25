import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('signing_setup',Path(__file__).resolve().parents[1]/'setup-updater-signing.py')
setup=importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)

class SigningSetup(unittest.TestCase):
    def test_existing_repository_key_is_never_rotated_and_all_pages_are_checked(self):
        with tempfile.TemporaryDirectory() as root:
            key=Path(root)/'permanent.key'
            with patch.object(sys,'argv',['setup','--key-file',str(key)]),patch.object(setup.subprocess,'run',return_value=subprocess.CompletedProcess([],0,'OTHER\nTAURI_UPDATER_PUBLIC_KEY\n','')) as run:
                with self.assertRaisesRegex(RuntimeError,'Refusing to rotate'):setup.main()
                self.assertIn('--paginate',run.call_args.args[0])
                self.assertIn('--jq',run.call_args.args[0])
                self.assertEqual(run.call_count,1)
                self.assertFalse(key.exists())

    def test_missing_administration_access_stops_before_key_generation(self):
        with tempfile.TemporaryDirectory() as root:
            key=Path(root)/'permanent.key'
            with patch.object(sys,'argv',['setup','--key-file',str(key)]),patch.object(setup.subprocess,'run',return_value=subprocess.CompletedProcess([],1,'','403')) as run:
                with self.assertRaisesRegex(RuntimeError,'administration access'):setup.main()
                self.assertEqual(run.call_count,1)
                self.assertFalse(key.exists())

    def test_local_backup_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as root:
            key=Path(root)/'permanent.key';key.write_bytes(b'existing private backup')
            with patch.object(sys,'argv',['setup','--key-file',str(key)]),patch.object(setup.subprocess,'run') as run:
                with self.assertRaisesRegex(RuntimeError,'never overwritten'):setup.main()
                run.assert_not_called()
                self.assertEqual(key.read_bytes(),b'existing private backup')
