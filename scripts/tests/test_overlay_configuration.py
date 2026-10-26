import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

class OverlayConfiguration(unittest.TestCase):
    def test_display_only_window_is_initially_hidden_and_never_focusable(self):
        config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
        window = next(w for w in config['app']['windows'] if w['label'] == 'overlay')
        for flag in ('visible', 'focus', 'focusable', 'resizable', 'decorations'):
            self.assertIs(window[flag], False, flag)
        self.assertTrue(window['alwaysOnTop'])
        self.assertTrue(window['skipTaskbar'])
        self.assertEqual(window['url'], 'overlay.html')
        self.assertFalse(config['app'].get('macOSPrivateApi', False))

    def test_overlay_can_read_only_body_free_status_not_mutate_or_read_recordings(self):
        overlay = json.loads((ROOT / 'src-tauri/capabilities/overlay.json').read_text())
        main = json.loads((ROOT / 'src-tauri/capabilities/default.json').read_text())
        self.assertEqual(overlay['windows'], ['overlay'])
        self.assertEqual(overlay['permissions'], ['allow-overlay-status'])
        self.assertNotIn('overlay', main['windows'])
        self.assertNotIn('remote', overlay)
