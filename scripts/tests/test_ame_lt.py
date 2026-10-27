import json,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from ame_lt import prompt_for,apply_spelling,validate_asr,parse_choice,parse_runtime_choice
class LTTest(unittest.TestCase):
    def test_image_prompt_has_no_authored_hint_or_expected_answer(self):
        a=prompt_for('雨がすごいですね。','image',{'id':'secret-scene','text':'答えは飴','expected':'飴'})
        b=prompt_for('雨がすごいですね。','image',{'id':'other','text':'答えは雨','expected':'雨'})
        self.assertEqual(a,b)
        self.assertNotIn('secret-scene',a);self.assertNotIn('答えは飴',a)
    def test_text_prompt_only_uses_authored_hint(self):
        a=prompt_for('雨がすごいですね。','text',{'text':'今日は台風だ。','expected':'飴','id':'secret'})
        self.assertIn('今日は台風だ。',a);self.assertNotIn('secret',a)
    def test_only_target_word_changes(self):
        self.assertEqual(apply_spelling('雨がすごいですね。','飴'),'飴がすごいですね。')
        self.assertEqual(apply_spelling('雨がすごいですね。','不明'),'雨がすごいですね。')
        for raw in ['雨が降っています。','雨と飴がすごいですね。','こんにちは']:
            with self.assertRaises(ValueError):validate_asr(raw)
    def test_malformed_or_expanded_output_cannot_rewrite_sentence(self):
        self.assertEqual(parse_choice('{"spelling":"飴"}'),'飴')
        for raw in ['{"spelling":"飴","text":"お祭りです"}','{"spelling":"お祭り"}','null']:
            with self.assertRaises(ValueError):parse_choice(raw)
    def test_text_runtime_marker_does_not_relax_json_validation(self):
        self.assertEqual(parse_runtime_choice('{"spelling":"雨"} [end of text]\n','text'),'雨')
        for raw in ['{"spelling":"雨"}', '説明 {"spelling":"雨"} [end of text]', '{"spelling":"雨"} {} [end of text]']:
            with self.assertRaises(ValueError):parse_runtime_choice(raw,'text')
if __name__=='__main__':unittest.main()
