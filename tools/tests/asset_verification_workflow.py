"""Static invariant for #75: 3D catalog verification is wired into a path-scoped check.

Plain Python (no Blender, no dependencies):

  python3 tools/tests/asset_verification_workflow.py --root "$PWD"

This inspects workflow/Makefile text only. It cannot prove that GitHub evaluates the
path filter as intended; that remains an independent review item in
docs/acceptance/asset-verification.md.
"""
import argparse
import re
import sys
from pathlib import Path

REQUIRED_PATHS = ('tools/build_assets.py', 'tools/tests/**', 'assets/3d/**')
BLENDER_VERSION = '5.0.1'
BLENDER_ARCHIVE_SHA256 = '8019580ee1b7262e505f4196a00237ccf743c88d205b38d34201510676e60b09'
BLENDER_WORDS = re.compile(r'(^|[\s/"\'$({])blender(\s|"|\'|$)|\$\{?BLENDER\b|blender-\d', re.IGNORECASE)


def logical_lines(text):
    """Join shell backslash continuations so one command is one line."""
    joined, pending = [], ''
    for raw in text.splitlines():
        line = raw.rstrip()
        if line.endswith('\\'):
            pending += line[:-1] + ' '
            continue
        joined.append(pending + line)
        pending = ''
    if pending:
        joined.append(pending)
    return joined


def strip_comment(line):
    return re.sub(r'(^|\s)#.*$', '', line)


def triggers(text):
    """Return {event: paths list or None (unscoped)} for the top-level `on:` block."""
    lines = text.splitlines()
    result = {}
    for index, line in enumerate(lines):
        match = re.match(r'^(on|"on"|\'on\'):\s*(.*)$', line)
        if not match:
            continue
        inline = strip_comment(match.group(2)).strip()
        if inline:
            for event in re.findall(r'[A-Za-z_]+', inline):
                result[event] = None
            return result
        event = None
        key = None
        for body in lines[index + 1:]:
            if body.strip() == '' or body.lstrip().startswith('#'):
                continue
            indent = len(body) - len(body.lstrip())
            if indent == 0:
                break
            content = strip_comment(body).strip()
            if indent == 2 and content.endswith(':'):
                event, key = content[:-1], None
                result[event] = None
            elif indent == 2:
                event, key = content.split(':')[0], None
                result[event] = None
            elif event and indent == 4 and content.endswith(':'):
                key = content[:-1]
                if key == 'paths':
                    result[event] = []
            elif event and key == 'paths' and content.startswith('- '):
                result[event].append(content[2:].strip().strip('\'"'))
        return result
    return result


def make_targets(text):
    targets, current = {}, None
    for line in logical_lines(text):
        match = re.match(r'^([A-Za-z0-9_.-]+):', line)
        if match and not line.startswith('\t'):
            current = match.group(1)
            targets[current] = []
        elif current and line.startswith('\t'):
            targets[current].append(line.strip())
    return targets


def blender_python_commands(lines):
    return [line for line in lines if '--python ' in line and BLENDER_WORDS.search(line)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    root = parser.parse_args().root.resolve()
    failures, notes = [], []
    workflows = sorted((root / '.github/workflows').glob('*.y*ml'))
    texts = {path: path.read_text() for path in workflows}

    # Unrelated PRs: no workflow may run Blender on an unscoped pull_request trigger.
    for path, text in texts.items():
        if any(BLENDER_WORDS.search(strip_comment(line)) for line in text.splitlines()):
            events = triggers(text)
            for event in ('pull_request', 'pull_request_target', 'push'):
                if event in events and events[event] is None:
                    failures.append(f'{path.name}: runs Blender on unscoped `{event}` (unrelated changes would pay for Blender)')

    makefile = (root / 'Makefile').read_text()
    targets = make_targets(makefile)
    verify_closure, pending = set(), ['verify']
    while pending:
        name = pending.pop()
        if name in verify_closure or name not in targets:
            continue
        verify_closure.add(name)
        for recipe in targets[name]:
            pending.extend(re.findall(r'\$\(MAKE\)\s+([A-Za-z0-9_.-]+)', recipe))
    for name in sorted(verify_closure):
        if any(BLENDER_WORDS.search(recipe) for recipe in targets[name]):
            failures.append(f'Makefile: `make verify` reaches Blender through `{name}`; the general gate must stay Blender-free')
    for name, recipes in sorted(targets.items()):
        for command in blender_python_commands(recipes):
            if '--python-exit-code 1' not in command:
                failures.append(f'Makefile `{name}`: Blender exits 0 when the script fails without --python-exit-code 1: {command}')

    asset_workflows = [path for path, text in texts.items()
                       if 'tools/build_assets.py' in text and '--validate' in text]
    if not asset_workflows:
        failures.append('no workflow runs `tools/build_assets.py -- --validate` (3D catalog validation is not wired)')
    for path in asset_workflows:
        text = texts[path]
        relative = path.relative_to(root).as_posix()
        events = triggers(text)
        pull_request = events.get('pull_request')
        if 'pull_request' not in events:
            failures.append(f'{path.name}: no pull_request trigger')
        elif pull_request is None:
            failures.append(f'{path.name}: pull_request is not path-scoped')
        else:
            required = list(REQUIRED_PATHS) + [relative]
            lines = logical_lines(text)
            if any(re.search(r'(^|\s)make\s', strip_comment(line)) for line in lines):
                required.append('Makefile')
            for item in required:
                if item not in pull_request:
                    failures.append(f'{path.name}: pull_request paths missing `{item}`')
        commands = [strip_comment(line) for line in logical_lines(text)]
        blender_commands = blender_python_commands(commands)
        for command in blender_commands:
            if '--python-exit-code 1' not in command:
                failures.append(f'{path.name}: Blender command does not fail closed (--python-exit-code 1): {command.strip()}')
        if not any('tools/build_assets.py' in c and '--validate' in c for c in blender_commands):
            failures.append(f'{path.name}: existing validator not run directly under Blender')
        attachment = [c for c in blender_commands if 'tools/tests/attachment_pose_blender.py' in c]
        if not any('--source' in c and '--generate' not in c and '--assets' not in c for c in attachment):
            failures.append(f'{path.name}: #73 contract not run on all ten saved sources/exports (--source, no --assets)')
        if not any('--generate' in c and '--source' in c and '--assets' not in c for c in attachment):
            failures.append(f'{path.name}: #73 contract not run with --generate --source on all ten animated candidates')
        if not any('tools/tests/asset_validation_negative.py' in c for c in blender_commands):
            failures.append(f'{path.name}: #75 negative harness not run')
        if not any('py_compile' in c and 'attachment_pose_blender.py' in c and 'asset_validation_negative.py' in c
                   for c in commands):
            notes.append(f'{path.name}: py_compile of the Blender test scripts not found (recommended)')
        if BLENDER_VERSION not in text:
            failures.append(f'{path.name}: Blender {BLENDER_VERSION} is not the pinned runtime')
        if BLENDER_ARCHIVE_SHA256 not in text or 'sha256sum' not in text:
            failures.append(f'{path.name}: Blender archive is not verified against SHA-256 {BLENDER_ARCHIVE_SHA256}')

    # The general Verify workflow must stay free of Blender work.
    verify = root / '.github/workflows/verify.yml'
    if verify.exists() and BLENDER_WORDS.search(verify.read_text()):
        failures.append('verify.yml: runs Blender for every pull request')

    for note in notes:
        print(f'note: {note}')
    for failure in failures:
        print(f'FAIL: {failure}')
    print(f'asset verification workflow invariant: {len(asset_workflows)} asset workflow(s), {len(failures)} failures')
    if failures:
        sys.exit(1)


if __name__ == '__main__':
    main()
