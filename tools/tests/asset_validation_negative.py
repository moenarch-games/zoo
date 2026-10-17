"""Zoo 3D catalog validation acceptance (#75): the real validator must reject broken outputs.

Run under the pinned Blender, only from the path-scoped 3D asset check:

  blender --background --factory-startup --python-exit-code 1 \
    --python tools/tests/asset_validation_negative.py -- --root "$PWD" --output /tmp/zoo75-negative

Each case copies the tracked development glTF/.bin packages, production GLBs and
catalog into a caller-owned disposable directory, injects exactly one defect into
that copy, redirects only the existing validator's destination constants there,
and runs the unchanged `validate_outputs()`. The unmodified copy must pass; every
injected defect must make validation fail with a diagnostic naming the defect.
Tracked files are hashed before and after and must be unchanged.
"""
import argparse
import hashlib
import importlib.util
import json
import re
import shutil
import struct
import sys
from pathlib import Path

import bpy

# Loading the authored validator must not leave tools/__pycache__ in the checkout.
sys.dont_write_bytecode = True

GENERATOR_CONSTANTS = ('ROOT', 'ASSETS', 'DEVELOPMENT', 'PRODUCTION', 'PREVIEWS', 'MANIFEST')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tracked_snapshot(root):
    """SHA-256 of everything verification must never mutate."""
    paths = [root / 'tools/build_assets.py', *sorted((root / 'assets/3d').rglob('*'))]
    return {path.relative_to(root).as_posix(): digest(path) for path in paths if path.is_file()}


def copy_catalog_tree(root, destination):
    source = root / 'assets/3d'
    target = destination / 'assets/3d'
    (target / 'production').mkdir(parents=True)
    shutil.copy2(source / 'catalog.json', target / 'catalog.json')
    for package in sorted((source / 'development').iterdir()):
        if package.is_dir():
            (target / 'development' / package.name).mkdir(parents=True)
            for path in sorted(package.iterdir()):
                if path.suffix in ('.gltf', '.bin'):
                    shutil.copy2(path, target / 'development' / package.name / path.name)
    for path in sorted((source / 'production').glob('*.glb')):
        shutil.copy2(path, target / 'production' / path.name)
    return target


# ---- glTF/GLB fixture editing (applied to the disposable copy only) ----

def read_gltf(assets, asset):
    path = assets / 'development' / asset / f'{asset}.gltf'
    return path, json.loads(path.read_text())


def gltf_bin(assets, asset, document):
    return assets / 'development' / asset / document['buffers'][0]['uri']


def read_glb(path):
    data = path.read_bytes()
    json_length = struct.unpack_from('<I', data, 12)[0]
    document = json.loads(data[20:20 + json_length].decode('utf8'))
    binary_header = 20 + json_length
    binary_length = struct.unpack_from('<I', data, binary_header)[0]
    binary = bytearray(data[binary_header + 8:binary_header + 8 + binary_length])
    return document, binary


def write_glb(path, document, binary):
    encoded = json.dumps(document, separators=(',', ':')).encode('utf8')
    encoded += b' ' * ((4 - len(encoded) % 4) % 4)
    binary = bytes(binary) + b'\0' * ((4 - len(binary) % 4) % 4)
    total = 12 + 8 + len(encoded) + 8 + len(binary)
    path.write_bytes(struct.pack('<4sII', b'glTF', 2, total)
                     + struct.pack('<I4s', len(encoded), b'JSON') + encoded
                     + struct.pack('<I4s', len(binary), b'BIN\x00') + binary)


def accessor_offset(document, index):
    accessor = document['accessors'][index]
    view = document['bufferViews'][accessor['bufferView']]
    return view.get('byteOffset', 0) + accessor.get('byteOffset', 0), accessor, view


def first_position(document):
    return document['meshes'][0]['primitives'][0]['attributes']['POSITION']


def clip(document, name):
    return next(animation for animation in document['animations'] if animation['name'] == name)


def freeze_samplers(document, binary, name):
    """Overwrite every key of every output in clip `name` with its first key."""
    components = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}
    for sampler in clip(document, name)['samplers']:
        start, accessor, view = accessor_offset(document, sampler['output'])
        size = components[accessor['type']] * 4
        stride = view.get('byteStride', size)
        first = bytes(binary[start:start + size])
        for key in range(1, accessor['count']):
            binary[start + key * stride:start + key * stride + size] = first


# ---- injected defects; each returns the exact copy paths it changed ----

def unchanged(assets):
    return []


def dev_missing_buffer(assets):
    path, document = read_gltf(assets, 'entrance')
    buffer = gltf_bin(assets, 'entrance', document)
    buffer.unlink()
    return [buffer]


def dev_truncated_buffer(assets):
    # External buffer shorter than its declared byteLength (last bytes lost).
    path, document = read_gltf(assets, 'entrance')
    buffer = gltf_bin(assets, 'entrance', document)
    data = buffer.read_bytes()
    buffer.write_bytes(data[:len(data) - 8])
    return [buffer]


def glb_corrupt_header(assets):
    path = assets / 'production/entrance.glb'
    data = bytearray(path.read_bytes())
    data[0:4] = b'XXXX'
    path.write_bytes(bytes(data))
    return [path]


def glb_truncated(assets):
    path = assets / 'production/entrance.glb'
    data = path.read_bytes()
    path.write_bytes(data[:len(data) - 64])
    return [path]


def dev_nonfinite_position(assets):
    path, document = read_gltf(assets, 'entrance')
    buffer = gltf_bin(assets, 'entrance', document)
    data = bytearray(buffer.read_bytes())
    start, _accessor, _view = accessor_offset(document, first_position(document))
    data[start:start + 4] = struct.pack('<f', float('nan'))
    buffer.write_bytes(bytes(data))
    return [buffer]


def prod_nonfinite_position(assets):
    path = assets / 'production/entrance.glb'
    document, binary = read_glb(path)
    start, _accessor, _view = accessor_offset(document, first_position(document))
    binary[start:start + 4] = struct.pack('<f', float('inf'))
    write_glb(path, document, binary)
    return [path]


def catalog_triangle_drift(assets):
    path = assets / 'catalog.json'
    catalog = json.loads(path.read_text())
    entry = next(item for item in catalog['assets'] if item['id'] == 'entrance')
    entry['triangles'] += 1
    path.write_text(json.dumps(catalog, indent=2) + '\n')
    return [path]


def over_budget_output(assets):
    # A real high-detail output (elephant, both stages) delivered for the 400-triangle
    # grass tile, with the catalog consistently recording the observed count, so only
    # the budget can reject it.
    development = assets / 'development/grass_tile'
    for path in development.iterdir():
        path.unlink()
    shutil.copy2(assets / 'development/elephant/elephant.gltf', development / 'grass_tile.gltf')
    shutil.copy2(assets / 'development/elephant/elephant.bin', development / 'elephant.bin')
    shutil.copy2(assets / 'production/elephant.glb', assets / 'production/grass_tile.glb')
    path = assets / 'catalog.json'
    catalog = json.loads(path.read_text())
    entries = {item['id']: item for item in catalog['assets']}
    entries['grass_tile']['triangles'] = entries['elephant']['triangles']
    path.write_text(json.dumps(catalog, indent=2) + '\n')
    return [development / 'grass_tile.gltf', development / 'elephant.bin',
            assets / 'production/grass_tile.glb', path]


def catalog_budget_exceeded(assets):
    # The catalog is the documented per-asset budget declaration; an entry whose
    # observed triangles exceed its declared triangleBudget is over budget.
    path = assets / 'catalog.json'
    catalog = json.loads(path.read_text())
    entry = next(item for item in catalog['assets'] if item['id'] == 'entrance')
    entry['triangleBudget'] = entry['triangles'] - 1
    path.write_text(json.dumps(catalog, indent=2) + '\n')
    return [path]


def dev_clip_without_motion(assets):
    path, document = read_gltf(assets, 'capybara')
    buffer = gltf_bin(assets, 'capybara', document)
    binary = bytearray(buffer.read_bytes())
    freeze_samplers(document, binary, 'walk')
    buffer.write_bytes(bytes(binary))
    return [buffer]


def prod_clip_without_motion(assets):
    path = assets / 'production/capybara.glb'
    document, binary = read_glb(path)
    freeze_samplers(document, binary, 'idle')
    write_glb(path, document, binary)
    return [path]


def prod_missing_clip(assets):
    path = assets / 'production/capybara.glb'
    document, binary = read_glb(path)
    document['animations'] = [animation for animation in document['animations']
                              if animation['name'] != 'walk']
    write_glb(path, document, binary)
    return [path]


def prod_single_channel_clip(assets):
    path = assets / 'production/capybara.glb'
    document, binary = read_glb(path)
    walk = clip(document, 'walk')
    walk['channels'] = walk['channels'][:1]
    write_glb(path, document, binary)
    return [path]


def dev_unimportable_mesh(assets):
    path, document = read_gltf(assets, 'entrance')
    for node in document['nodes']:
        node.pop('mesh', None)
    path.write_text(json.dumps(document, separators=(',', ':')))
    return [path]


# name -> (defect, required diagnostic regex or None for "must pass", what it proves).
# A failure only counts when it also occurs while validating the asset the defect
# was injected into (TARGETS below).
CASES = {
    'unmodified_copy': (unchanged, None,
                        'the redirected unchanged validator accepts the tracked catalog'),
    'dev_missing_buffer': (dev_missing_buffer, r'missing buffer: .*entrance',
                           'a development glTF whose external .bin is absent fails'),
    'dev_truncated_buffer': (dev_truncated_buffer, r'missing buffer|buffer is smaller|byteLength|truncat',
                             'an external .bin shorter than its declared byteLength fails'),
    'glb_corrupt_header': (glb_corrupt_header, r'invalid GLB: .*entrance\.glb',
                           'a production GLB with a corrupted magic header fails'),
    'glb_truncated': (glb_truncated, r'invalid GLB: .*entrance\.glb',
                      'a production GLB shorter than its header length fails'),
    'dev_nonfinite_position': (dev_nonfinite_position, r'non-finite positions: .*entrance\.gltf',
                               'a NaN POSITION value in a development buffer fails'),
    'prod_nonfinite_position': (prod_nonfinite_position, r'non-finite positions: .*entrance\.glb',
                                'an infinite POSITION value in a production GLB fails'),
    'catalog_triangle_drift': (catalog_triangle_drift, r'triangle drift: entrance',
                               'catalog triangle counts must match both actual outputs'),
    'over_budget_output': (over_budget_output, r'triangle budget exceeded: grass_tile',
                           'a consistent but over-budget actual output fails the budget'),
    'catalog_budget_exceeded': (catalog_budget_exceeded, r'budget.*entrance|entrance.*budget',
                                'an output exceeding its catalog-declared triangleBudget fails'),
    'dev_clip_without_motion': (dev_clip_without_motion, r'animation has no motion: .*capybara\.gltf:walk',
                                'a development walk clip whose keys are all frozen fails'),
    'prod_clip_without_motion': (prod_clip_without_motion, r'animation has no motion: .*capybara\.glb:idle',
                                 'a production idle clip whose keys are all frozen fails'),
    'prod_missing_clip': (prod_missing_clip, r'production clips: .*capybara\.glb',
                          'a production animal missing its walk clip fails'),
    'prod_single_channel_clip': (prod_single_channel_clip, r'incomplete animation channels: .*capybara\.glb',
                                 'a production walk clip reduced to one channel fails'),
    'dev_unimportable_mesh': (dev_unimportable_mesh, r'failed to load mesh: .*entrance\.gltf',
                              'a development package with no importable mesh node fails'),
}


def load_validator(root):
    spec = importlib.util.spec_from_file_location('zoo_catalog_validator', root / 'tools/build_assets.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def redirect(validator, case_root):
    """Only destination constants change; validation code is the authored code."""
    validator.ROOT = case_root
    validator.ASSETS = case_root / 'assets/3d'
    validator.DEVELOPMENT = validator.ASSETS / 'development'
    validator.PRODUCTION = validator.ASSETS / 'production'
    validator.PREVIEWS = validator.ASSETS / 'previews'
    validator.MANIFEST = validator.ASSETS / 'catalog.json'
    for name in GENERATOR_CONSTANTS:
        target = Path(getattr(validator, name)).resolve()
        assert target == case_root or case_root in target.parents, f'{name} escaped {case_root}'


# The asset each defect was injected into.
TARGETS = {
    'unmodified_copy': None,
    'dev_missing_buffer': 'entrance', 'dev_truncated_buffer': 'entrance',
    'glb_corrupt_header': 'entrance', 'glb_truncated': 'entrance',
    'dev_nonfinite_position': 'entrance', 'prod_nonfinite_position': 'entrance',
    'catalog_triangle_drift': 'entrance', 'over_budget_output': 'grass_tile',
    'catalog_budget_exceeded': 'entrance',
    'dev_clip_without_motion': 'capybara', 'prod_clip_without_motion': 'capybara',
    'prod_missing_clip': 'capybara', 'prod_single_channel_clip': 'capybara',
    'dev_unimportable_mesh': 'entrance',
}
assert set(TARGETS) == set(CASES)


def failing_asset(error, validator_path):
    """The asset `validate_outputs()` was checking when it raised."""
    asset = None
    trace = error.__traceback__
    while trace is not None:
        frame = trace.tb_frame
        if Path(frame.f_code.co_filename).resolve() == validator_path and frame.f_code.co_name == 'validate_outputs':
            asset = frame.f_locals.get('asset')
        trace = trace.tb_next
    return asset


def summary(error):
    lines = [line.strip() for line in str(error).strip().splitlines() if line.strip()]
    if not lines:
        return type(error).__name__
    text = f'{type(error).__name__}: {lines[0]}'
    return text if len(lines) == 1 else f'{text} ... {lines[-1]}'


def run_case(name, root, output, validator):
    defect, expected, proves = CASES[name]
    case_root = (output / 'cases' / name).resolve()
    if case_root.exists():
        assert output in case_root.parents and case_root.name == name
        shutil.rmtree(case_root)
    assets = copy_catalog_tree(root, case_root)
    changed = [path.relative_to(case_root).as_posix() for path in defect(assets)]
    redirect(validator, case_root)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    observed, diagnostic, message, asset = 'passed', '', '', None
    try:
        validator.validate_outputs()
    except Exception as error:  # every exception fails closed; attribution is checked below
        observed, diagnostic, message = 'failed', summary(error), f'{type(error).__name__}: {error}'
        asset = failing_asset(error, (root / 'tools/build_assets.py').resolve())
    if expected is None:
        ok = observed == 'passed'
    else:
        ok = observed == 'failed' and asset == TARGETS[name] and re.search(expected, message) is not None
    return {'case': name, 'proves': proves, 'changed': changed,
            'expected': 'pass' if expected is None else f'fail on {TARGETS[name]} matching /{expected}/',
            'observed': observed if observed == 'passed' else f'failed on {asset}',
            'diagnostic': diagnostic[:400], 'as_expected': ok}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--cases', nargs='+', choices=sorted(CASES), default=list(CASES))
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    root = args.root.resolve()
    output = args.output.resolve()
    if output == root or root in output.parents or output in root.parents or output == Path(output.anchor):
        raise AssertionError(f'output must be a caller-owned directory outside the checkout: {output}')
    output.mkdir(parents=True, exist_ok=True)
    before = tracked_snapshot(root)
    validator = load_validator(root)
    rows = []
    for name in args.cases:
        row = run_case(name, root, output, validator)
        rows.append(row)
        print(f"[{'OK' if row['as_expected'] else 'UNEXPECTED'}] {name}: expected {row['expected']}; "
              f"observed {row['observed']} {row['diagnostic']}")
    after = tracked_snapshot(root)
    mutated = sorted(path for path in set(before) | set(after) if before.get(path) != after.get(path))
    report = {'blender': bpy.app.version_string, 'generator_sha256': before['tools/build_assets.py'],
              'tracked_mutations': mutated, 'results': rows}
    (output / 'asset-validation-negative-report.json').write_text(json.dumps(report, indent=2) + '\n')
    unexpected = [row['case'] for row in rows if not row['as_expected']]
    print(f'asset validation acceptance: {len(rows)} cases, {len(rows) - len(unexpected)} as expected, '
          f'{len(unexpected)} unexpected, {len(mutated)} tracked mutations')
    if mutated:
        raise AssertionError(f'verification mutated tracked files: {mutated}')
    if unexpected:
        raise AssertionError(f'unexpected validator outcomes: {unexpected}')


if __name__ == '__main__':
    main()
