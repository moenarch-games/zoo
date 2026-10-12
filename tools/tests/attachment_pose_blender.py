"""Zoo content acceptance through actual Blender import and named clip evaluation.

Run only for the affected source/export path, not ordinary unrelated verification.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector

HUMANS = ('guest', 'keeper', 'janitor', 'mechanic')
QUADRUPEDS = ('capybara', 'zebra', 'giraffe', 'elephant')
ANIMATED = HUMANS + QUADRUPEDS + ('flamingo', 'penguin')
FRAMES = (1, 7, 13, 19, 25)
# Exported single-precision transforms: tolerance is geometric precision, not timing.
EPSILON = 2e-5


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def select_clip(name):
    found = False
    for obj in bpy.context.scene.objects:
        animation = obj.animation_data
        if animation is None:
            continue
        strips = [strip for track in animation.nla_tracks for strip in track.strips
                  if track.name == name or strip.name == name]
        animation.action = None
        for track in animation.nla_tracks:
            track.mute = True
        if strips:
            strip = strips[0]
            animation.action = strip.action
            animation.action_slot = strip.action_slot
            found = True
    if not found:
        raise AssertionError(f'actual imported clip unavailable: {name}')
    bpy.context.scene.frame_set(1)
    bpy.context.view_layer.update()


def matrix_error(left, right):
    return max(abs(left[row][col] - right[row][col])
               for row in range(4) for col in range(4))


def descendants(obj):
    result = set()
    while obj.parent:
        obj = obj.parent
        result.add(obj.name)
    return result


def pairs(asset):
    if asset in HUMANS:
        return [('hand_-1', 'arm_left'), ('hand_1', 'arm_right'),
                ('shoe_-1', 'leg_left'), ('shoe_1', 'leg_right')]
    if asset in QUADRUPEDS:
        return [(f'hoof_{index}', f'leg_{index}') for index in range(4)]
    if asset == 'flamingo':
        return [(f'foot_{side}', f'leg_{side}') for side in (-1, 1)]
    return []


def face_parts(asset):
    names = [obj.name for obj in bpy.context.scene.objects]
    prefixes = ('eye_', 'pupil_', 'ear_', 'tusk_', 'capybara_cheek_', 'ossicone_')
    return [name for name in names if name.startswith(prefixes)
            or name in ('nose', 'muzzle', 'beak', 'cap')]


def render(path):
    scene = bpy.context.scene
    bpy.ops.object.camera_add(location=(3.2, 4.8, 2.8))
    camera = bpy.context.object
    camera.rotation_euler = (Vector((0, 0, 1)) - camera.location).to_track_quat('-Z', 'Y').to_euler()
    scene.camera = camera
    scene.render.engine = 'BLENDER_WORKBENCH'
    scene.display.shading.light = 'STUDIO'
    scene.display.shading.color_type = 'MATERIAL'
    scene.display.shading.show_cavity = True
    scene.render.resolution_x = 480
    scene.render.resolution_y = 480
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(camera, do_unlink=True)


def inspect(path, asset, output, pictures):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if path.suffix == '.blend':
        bpy.ops.wm.open_mainfile(filepath=str(path))
        # Source actions are independently authored per moving object, before export merges.
        for obj in bpy.context.scene.objects:
            if obj.animation_data and obj.animation_data.action:
                action = obj.animation_data.action
                track = obj.animation_data.nla_tracks.new()
                track.name = 'walk' if action.name.startswith('walk') else 'idle'
                strip = track.strips.new(track.name, 1, action)
                strip.action_slot = obj.animation_data.action_slot
    else:
        bpy.ops.import_scene.gltf(filepath=str(path))
    root = bpy.data.objects['character_root' if asset in HUMANS else 'animal_root']
    objects = {obj.name: obj for obj in bpy.context.scene.objects}
    result = {'path': str(path), 'sha256': digest(path), 'asset': asset,
              'rest_finite': True, 'rest_loop_error': 0.0, 'failures': [], 'observations': []}
    if path.suffix == '.gltf':
        document = json.loads(path.read_text())
        result['external_buffers'] = [{ 'path': buffer['uri'],
                                        'sha256': digest(path.parent / buffer['uri']) }
                                      for buffer in document.get('buffers', [])]
    for obj in objects.values():
        if obj.type == 'MESH':
            if not all(math.isfinite(value)
                       for vertex in obj.data.vertices for value in obj.matrix_world @ vertex.co):
                result['rest_finite'] = False
    # Secondary facial motion (the elephant trunk) may change its own local pose,
    # but must still inherit the head. A neck/head chain likewise retains ancestry.
    required_chains = [('trunk', 'head')] if asset == 'elephant' else []
    if asset in ('giraffe', 'flamingo'):
        required_chains.append(('head', 'neck'))
    for detail_name, parent_name in required_chains:
        if parent_name not in descendants(objects[detail_name]):
            result['failures'].append(f'{detail_name} must inherit {parent_name}; independent secondary motion is allowed')
    for clip in ('idle', 'walk'):
        select_clip(clip)
        rest_pose = {name: obj.matrix_world.copy() for name, obj in objects.items()}
        attachments = pairs(asset) if clip == 'walk' else [(name, 'head') for name in face_parts(asset)]
        for detail_name, limb_name in attachments:
            detail, limb = objects[detail_name], objects[limb_name]
            rest = limb.matrix_world.inverted() @ detail.matrix_world
            initial = root.matrix_world.inverted() @ limb.matrix_world
            # The proximal material point is identified from real rest geometry, not a future joint name.
            vertices = [vertex.co for vertex in limb.data.vertices]
            proximal = Vector(((min(v.x for v in vertices) + max(v.x for v in vertices)) / 2,
                               (min(v.y for v in vertices) + max(v.y for v in vertices)) / 2,
                               max(v.z for v in vertices)))
            anchor = initial @ proximal
            maximum_relative = maximum_pivot = maximum_motion = 0.0
            for frame in FRAMES:
                bpy.context.scene.frame_set(frame)
                bpy.context.view_layer.update()
                relative = limb.matrix_world.inverted() @ detail.matrix_world
                current = root.matrix_world.inverted() @ limb.matrix_world
                maximum_relative = max(maximum_relative, matrix_error(rest, relative))
                maximum_pivot = max(maximum_pivot, (current @ proximal - anchor).length)
                maximum_motion = max(maximum_motion, matrix_error(initial, current))
            observation = {'clip': clip, 'detail': detail_name, 'part': limb_name,
                           'relative_error': maximum_relative, 'proximal_drift': maximum_pivot,
                           'part_motion': maximum_motion,
                           'inherits_part': limb_name in descendants(detail)}
            result['observations'].append(observation)
            if maximum_relative > EPSILON or limb_name not in descendants(detail):
                result['failures'].append(f'{clip}: {detail_name} must inherit and remain attached to {limb_name}')
            if maximum_motion <= EPSILON:
                result['failures'].append(f'{clip}: actual {limb_name} intermediate motion missing')
            if clip == 'walk' and maximum_pivot > EPSILON:
                result['failures'].append(f'{clip}: {limb_name} rotates around its center instead of anchored proximal joint')
            bpy.context.scene.frame_set(1)
        if asset == 'penguin' and clip == 'walk':
            for name in ('wing_-1', 'wing_1'):
                limb = objects[name]
                bpy.context.scene.frame_set(1)
                initial = root.matrix_world.inverted() @ limb.matrix_world
                vertices = [vertex.co for vertex in limb.data.vertices]
                proximal = Vector(((min(v.x for v in vertices) + max(v.x for v in vertices)) / 2,
                                   (min(v.y for v in vertices) + max(v.y for v in vertices)) / 2,
                                   max(v.z for v in vertices)))
                anchor = initial @ proximal
                drift = motion = 0.0
                for frame in FRAMES:
                    bpy.context.scene.frame_set(frame)
                    current = root.matrix_world.inverted() @ limb.matrix_world
                    drift = max(drift, (current @ proximal - anchor).length)
                    motion = max(motion, matrix_error(initial, current))
                result['observations'].append({'clip': clip, 'part': name,
                                               'proximal_drift': drift, 'part_motion': motion})
                if drift > EPSILON:
                    result['failures'].append(f'{clip}: {name} must rotate around its anchored shoulder')
                if motion <= EPSILON:
                    result['failures'].append(f'{clip}: actual {name} motion missing')
        bpy.context.scene.frame_set(25)
        bpy.context.view_layer.update()
        loop_error = max(matrix_error(rest_pose[name], obj.matrix_world) for name, obj in objects.items())
        result['rest_loop_error'] = max(result['rest_loop_error'], loop_error)
        if loop_error > EPSILON:
            result['failures'].append(f'{clip}: final actual pose does not return to rest')
        if pictures:
            for frame in (1, 13):
                bpy.context.scene.frame_set(frame)
                render(output / f'{asset}-{path.suffix[1:]}-{clip}-{frame}.png')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--assets', nargs='+', default=ANIMATED)
    parser.add_argument('--pictures', action='store_true')
    parser.add_argument('--source', action='store_true')
    parser.add_argument('--generate', action='store_true', help='execute the real source generator into output/generated')
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    args.output.mkdir(parents=True, exist_ok=True)
    generator_path = args.root / 'tools/build_assets.py'
    generator_hash = digest(generator_path)
    inspected_root = args.root
    if args.generate:
        spec = importlib.util.spec_from_file_location('zoo_source_generator', generator_path)
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        inspected_root = args.output / 'generated'
        generator.ROOT = inspected_root
        generator.ASSETS = inspected_root / 'assets/3d'
        generator.DEVELOPMENT = generator.ASSETS / 'development'
        generator.PRODUCTION = generator.ASSETS / 'production'
        generator.PREVIEWS = generator.ASSETS / 'previews'
        for stage in ('development', 'production'):
            for asset in args.assets:
                generator.export(asset, stage)
        if digest(generator_path) != generator_hash:
            raise AssertionError('verification changed authored generator')
    rows = []
    for asset in args.assets:
        paths = [inspected_root / 'assets/3d/development' / asset / f'{asset}.gltf',
                 inspected_root / 'assets/3d/production' / f'{asset}.glb']
        if args.source:
            paths.append(inspected_root / 'assets/3d/development' / asset / f'{asset}.blend')
        for path in paths:
            rows.append(inspect(path, asset, args.output, args.pictures))
    report = {'blender': bpy.app.version_string, 'frames': FRAMES,
              'generator_sha256': generator_hash, 'generated_from_source': args.generate, 'results': rows}
    (args.output / 'attachment-report.json').write_text(json.dumps(report, indent=2) + '\n')
    failures = [failure for row in rows for failure in row['failures']]
    if not all(row['rest_finite'] for row in rows):
        failures.append('nonfinite real rest geometry')
    print(f'attachment acceptance: {len(rows)} actual sources/exports, {len(failures)} failures')
    if failures:
        raise AssertionError('\n'.join(failures))


if __name__ == '__main__':
    main()
