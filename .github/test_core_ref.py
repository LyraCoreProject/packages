import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[1]


class PairedCore(unittest.TestCase):
    def select(self, branches, head, base='', status=2):
        action = (ROOT / '.github/actions/core-ref/action.yml').read_text()
        script = textwrap.dedent(action.split('      run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            git = directory / 'git'
            git.write_text('#!/bin/bash\nfor pair in $PAIRS; do\n'
                           '  if [[ "${@: -1}" == "refs/heads/${pair%=*}" ]]; then\n'
                           '    echo "${pair#*=} ${@: -1}"; exit 0\n'
                           '  fi\ndone\nexit "$MISSING_STATUS"\n')
            git.chmod(0o755)
            output = directory / 'output'
            result = subprocess.run(['bash', '-euc', script], env={
                **os.environ, 'PATH': f'{directory}:{os.environ["PATH"]}',
                'BRANCH': head, 'BASE_BRANCH': base, 'FALLBACK': 'required', 'GITHUB_OUTPUT': str(output),
                'PAIRS': ' '.join(f'{k}={v}' for k, v in branches.items()),
                'MISSING_STATUS': str(status),
            }, capture_output=True, text=True)
            return result.returncode, output.read_text() if output.exists() else ''

    def test_exact_partner_wins_over_base(self):
        self.assertEqual(self.select({'slice': 'abc', 'base': 'def'}, 'slice', 'base'),
                         (0, 'ref=abc\n'))

    def test_slice_uses_its_base_partner(self):
        self.assertEqual(self.select({'base': 'def'}, 'slice', 'base'), (0, 'ref=def\n'))

    def test_unpaired_vmangos_stack_keeps_the_required_revision(self):
        self.assertEqual(self.select({'feat/vmangos-world-data': 'abc'},
                                     'vmangos/second', 'vmangos/first'), (0, 'ref=required\n'))

    def test_unpaired_branch_into_main_keeps_the_required_revision(self):
        self.assertEqual(self.select({'main': 'moving-tip'}, 'unpaired', 'main'), (0, 'ref=required\n'))

    def test_remote_failure_does_not_select_main(self):
        self.assertEqual(self.select({}, 'slice', status=128), (128, ''))

    def test_main_keeps_the_required_revision(self):
        self.assertEqual(self.select({"main": "abc"}, "main"), (0, "ref=required\n"))

    def test_pairing_is_only_for_non_main_bases(self):
        self.assertEqual(self.select({'slice': 'paired', 'main': 'moving-tip'}, 'slice', 'main'),
                         (0, 'ref=required\n'))
