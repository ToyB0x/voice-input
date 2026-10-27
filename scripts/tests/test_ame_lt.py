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
    def test_text_transport_accepts_one_response_only(self):
        banner = ('\n Running in chat mode, available commands:\n'
                  '   /image <path>    load an image\n'
                  '   /clear           clear the chat history\n'
                  '   /quit or /exit   exit the program\n\n> ')
        response = '{"spelling":"飴"}'
        self.assertEqual(parse_runtime_choice(banner+response+'\n\n> \n\n','text'),'飴')
        for output in [banner+response+'\n> '+response+'\n> ', banner+'説明'+response+'\n> ',response]:
            with self.assertRaises(ValueError):parse_runtime_choice(output,'text')
if __name__=='__main__':unittest.main()
