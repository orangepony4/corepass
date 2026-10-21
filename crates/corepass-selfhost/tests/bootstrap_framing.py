"""Offline transport framing checks; executes a benign fixture, never controller.py."""
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest

SOURCE = Path(__file__).resolve().parents[1] / 'src/lib.rs'
BOOTSTRAP = re.search(r'const PYTHON_BOOTSTRAP: &str = r#"(.*?)"#;', SOURCE.read_text(encoding='utf8'), re.S).group(1)


class BootstrapTests(unittest.TestCase):
    def run_fixture(self, source, request, *, length=None):
        encoded = source.encode('utf8')
        payload = str(len(encoded) if length is None else length).encode('ascii') + b'\n' + encoded + json.dumps(request, ensure_ascii=False).encode('utf8') + b'\n'
        return subprocess.run([sys.executable, '-c', BOOTSTRAP], input=payload,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)

    def test_shared_buffer_keeps_json_after_unicode_source(self):
        source = '# пример UTF-8\nimport json,sys\nprint(json.dumps({"name":__name__,"request":json.loads(sys.stdin.buffer.read())},ensure_ascii=False))\n'
        request = {'action': 'fixture', 'name': "телефон ' ; $(echo must-not-run)"}
        result = self.run_fixture(source, request)
        self.assertEqual(result.returncode, 0)
        value = json.loads(result.stdout)
        self.assertEqual(value, {'name': '__main__', 'request': request})
        self.assertEqual(result.stderr, b'')

    def test_truncated_source_is_not_executed(self):
        result = self.run_fixture('print("must-not-run")\n', {}, length=9999)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'')

    def test_command_stays_short_and_has_no_shell_quote(self):
        self.assertLess(len(BOOTSTRAP), 512)
        self.assertNotIn("'", BOOTSTRAP)


if __name__ == '__main__':
    unittest.main()
