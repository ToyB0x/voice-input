import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

class OverlayConfiguration(unittest.TestCase):
    def test_overlay_adds_no_renderer_or_private_macos_api(self):
        config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
        self.assertEqual([w['label'] for w in config['app']['windows']], ['main'])
        self.assertFalse(config['app'].get('macOSPrivateApi', False))
        self.assertFalse((ROOT / 'overlay.html').exists())

    def test_native_panel_has_no_ipc_capability_surface(self):
        for path in (ROOT / 'src-tauri/capabilities').glob('*.json'):
            capability = json.loads(path.read_text())
            self.assertNotIn('overlay', capability.get('windows', []))
            self.assertNotIn('allow-overlay-status', capability.get('permissions', []))
