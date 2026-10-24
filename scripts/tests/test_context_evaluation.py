import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from evaluate_context import compare


class ContextEvaluationTests(unittest.TestCase):
    def test_contamination_and_silence_are_reported_independently_of_cer(self):
        corpus = {'cases': [dict(id='spoken', reference='東京', hint='東京', unrelated_hint='葛飾北斎'),
                            dict(id='silence', reference='', hint='東京', unrelated_hint='葛飾北斎')]}
        def report(spoken, silence):
            return {'cases': [dict(id=name, audio_seconds=2,
                observations=[dict(text=text, inference_ms=10), dict(text=text, inference_ms=11)])
                for name, text in [('spoken', spoken), ('silence', silence)]]}
        result = compare(corpus, dict(none=report('東', ''), related=report('東京', ''),
                                     unrelated=report('東京 葛飾北斎', '葛飾北斎')))
        self.assertEqual(result['metrics']['none']['cer'], .5)
        self.assertEqual(result['metrics']['related']['cer'], 0)
        self.assertEqual(result['metrics']['unrelated']['silence_false_positives'], 1)
        self.assertEqual(result['unrelated_canary_cases'], ['spoken', 'silence'])
        self.assertEqual(result['changed_by_related_hint'], ['spoken'])
        self.assertEqual(len(result['cases'][0]['modes']['none']['observations']), 2)


if __name__ == '__main__':
    unittest.main()
