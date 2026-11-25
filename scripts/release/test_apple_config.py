"""The release builds macOS only with complete Apple signing, else Windows alone."""
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/release/apple_config.sh'
WORKFLOW = ROOT / '.github/workflows/release.yml'
KEYS = (
    'APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_SIGNING_IDENTITY',
    'APPLE_TEAM_ID', 'APPLE_API_ISSUER', 'APPLE_API_KEY', 'APPLE_API_KEY_PATH',
)


def run(values):
    with tempfile.NamedTemporaryFile('r', delete=False) as output:
        path = output.name
    try:
        env = {k: v for k, v in os.environ.items() if k not in KEYS}
        env.update(values)
        env['GITHUB_OUTPUT'] = path
        result = subprocess.run(['bash', str(SCRIPT)], env=env, capture_output=True, text=True)
        return result.returncode, Path(path).read_text(), result.stdout + result.stderr
    finally:
        os.unlink(path)


class AppleConfigurationTests(unittest.TestCase):
    def test_all_apple_secrets_enable_macos(self):
        code, output, _ = run({key: 'value' for key in KEYS})
        self.assertEqual(code, 0)
        self.assertEqual(output.strip(), 'enabled=true')

    def test_no_apple_secrets_release_windows_alone(self):
        code, output, log = run({})
        self.assertEqual(code, 0)
        self.assertEqual(output.strip(), 'enabled=false')
        self.assertIn('Windows', log)

    def test_any_partial_apple_configuration_is_refused(self):
        for missing in KEYS:
            with self.subTest(missing=missing):
                code, output, log = run({key: 'value' for key in KEYS if key != missing})
                self.assertNotEqual(code, 0)
                self.assertEqual(output, '')
                self.assertIn(missing, log)
        code, output, _ = run({'APPLE_TEAM_ID': 'value'})
        self.assertNotEqual(code, 0)
        self.assertEqual(output, '')

    def test_macos_job_runs_only_when_apple_signing_is_complete(self):
        workflow = WORKFLOW.read_text()
        macos = workflow.split('\n  macos:\n', 1)[1].split('\n  windows:\n', 1)[0]
        self.assertRegex(macos, r"(?m)^    if: \$\{\{ needs\.gate\.outputs\.macos == 'true' \}\}$")
        gate = workflow.split('\n  gate:\n', 1)[1].split('\n  ci:\n', 1)[0]
        self.assertIn('macos: ${{ steps.apple.outputs.enabled }}', gate)
        self.assertIn('run: bash scripts/release/apple_config.sh', gate)
