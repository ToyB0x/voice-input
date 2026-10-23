import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from evaluate_models import distance, normalize, score, summarize


class EvaluationTests(unittest.TestCase):
    def test_unicode_normalization_is_fixed_but_not_phonetic(self):
        self.assertEqual(normalize('ＡＢＣ、 Wi-Fi！ ガ\u3000ク'), 'abcwifiガク')
        self.assertNotEqual(normalize('三人'), normalize('3人'))
        self.assertNotEqual(normalize('月日'), normalize('月皮'))

    def test_character_errors_include_insert_delete_substitute(self):
        self.assertEqual(distance('東京駅', '東京'), 1)
        self.assertEqual(distance('東京', '東京都'), 1)
        self.assertEqual(distance('月日', '月皮'), 1)
        self.assertEqual(score('あ', 'あいう')['cer'], 2.0)
        self.assertIsNone(score('', '幻覚')['cer'])
        self.assertEqual(score('', '幻覚')['edits'], 2)

    def test_aggregate_weights_reference_characters_and_excludes_silence_latency(self):
        cases = [dict(reference='あ', hypothesis='い', normalized=score('あ', 'い'), inference_ms=10, audio_seconds=1),
                 dict(reference='あ'*9, hypothesis='あ'*9, normalized=score('あ'*9, 'あ'*9), inference_ms=30, audio_seconds=1),
                 dict(reference='', hypothesis='幻', normalized=score('', '幻'), inference_ms=999, audio_seconds=2)]
        result = summarize(cases)
        self.assertEqual(result['cer'], 0.1)
        self.assertEqual(result['median_inference_ms'], 20)
        self.assertEqual(result['rtf'], 0.02)
        self.assertEqual(result['silence_false_positives'], 1)

class ComparisonCompletenessTests(unittest.TestCase):
    def reports(self):
        from transcription_models import MODELS
        return [dict(model_name=model, backend=backend, model_sha256=MODELS[model]['sha256'],
                     corpus_sha256='same-corpus', source_revision='same-code', protocol_version='ja-cer-v1', cargo_lock_sha256='same-lock', network_denied_probe=True, model_bytes=MODELS[model]['bytes'],
                     summary=dict(cer=0.1, silence_false_positives=0))
                for model in ['base', 'small'] for backend in ['cpu', 'metal']]

    def test_partial_and_mixed_corpus_results_are_rejected(self):
        from summarize_evaluation import validate
        reports = self.reports()
        with self.assertRaises(ValueError): validate(reports[:-1], ['base', 'small'])
        with self.assertRaises(ValueError): validate(reports + reports[:1], ['base', 'small'])
        reports[0]['corpus_sha256'] = 'different-corpus'
        with self.assertRaises(ValueError): validate(reports, ['base', 'small'])
        reports[0]['corpus_sha256'] = 'same-corpus'
        reports[0]['source_revision'] = 'different-code'
        with self.assertRaises(ValueError): validate(reports, ['base', 'small'])

    def test_unverified_offline_results_cannot_be_presented_as_offline(self):
        from summarize_evaluation import validate
        reports = self.reports()
        self.assertEqual(len(validate(reports, ['base', 'small'], True)), 4)
        reports[0]['network_denied_probe'] = False
        with self.assertRaises(ValueError): validate(reports, ['base', 'small'], True)


if __name__ == '__main__':
    unittest.main()
