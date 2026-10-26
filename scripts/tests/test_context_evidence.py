import unittest
from context_evidence import judge, sources

class ContextEvidenceTests(unittest.TestCase):
    def test_no_credit_for_kana_mixed_or_omitted_word(self):
        for value,expected in [('あめがすごいですね','undetermined'),('すごいですね','undetermined'),('雨と飴です','mixed'),('雨がすごいですね','target'),('飴がすごいですね','opposite')]:
            self.assertEqual(judge(value,'雨','飴'),expected)

    def test_failed_ax_is_not_replaced_with_fixture_and_labels_are_not_mapped(self):
        capture=dict(fixtureText='REFERENCE_CONTEXT',ax={'text':''},ocr={'text':'ACTUAL_OCR'},imageOCR={'text':''},vision={'status':'completed','labels':[dict(label='actual-label',confidence=.8)]})
        result=sources(capture,{'fixtureText':'UNRELATED'})
        self.assertEqual(result['ax'],'')
        self.assertEqual(result['fixture-text'],'REFERENCE_CONTEXT')
        self.assertEqual(result['ocr'],'ACTUAL_OCR')
        self.assertEqual(result['image-ocr'],'')
        self.assertEqual(result['vision-labels'],'actual-label (0.800)')
        self.assertEqual(result['mismatched-text'],'UNRELATED')
        capture['vision']['status']='unavailable'
        self.assertEqual(sources(capture,{'fixtureText':'x'})['vision-labels'],'')
