#!/usr/bin/env python3
"""Build Zoo's editable glTF packages and packed runtime GLBs.

Run from the repository root:
  blender --background --python tools/build_assets.py -- --stage all
  blender --background --python tools/build_assets.py -- --stage development
  blender --background --python tools/build_assets.py -- --stage production
  blender --background --python tools/build_assets.py -- --validate

All geometry is authored deterministically with Blender primitives.  The source
scene uses named transform parts, intentionally avoiding modifier-only or
generated-at-export topology so the development glTF remains easy to edit.
"""
import argparse
import json
import math
import os
import struct
import sys
from pathlib import Path

import bpy
import mathutils


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets" / "3d"
DEVELOPMENT = ASSETS / "development"
PRODUCTION = ASSETS / "production"
PREVIEWS = ASSETS / "previews"
MANIFEST = ASSETS / "catalog.json"

PALETTE = {
    "leaf": "#74ad50", "path": "#d1bb8d", "wood": "#38513c", "dark_wood": "#243f38",
    "cream": "#d8c79d", "trim": "#eadfbf", "roof": "#9d4937", "teal": "#315c58",
    "yellow": "#e0bf65", "food": "#e7b957", "drink": "#87c7d8", "white": "#f4efe2",
    "pink": "#e9859b", "black": "#262c31", "zebra": "#f5f0dc", "elephant": "#899a9d",
    "capybara": "#a87850", "giraffe": "#d49b4a", "penguin": "#34434c", "skin": "#e1b98d",
    "keeper": "#587f61", "janitor": "#5d8c62", "mechanic": "#537895", "crate": "#b7834e",
}


def args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage", choices=("development", "production", "all"), default="all")
    parser.add_argument("--validate", action="store_true")
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])


def material(name):
    value = PALETTE[name].lstrip("#")
    color = tuple(int(value[index:index + 2], 16) / 255 for index in (0, 2, 4)) + (1,)
    key = f"zoo_{name}"
    existing = bpy.data.materials.get(key)
    if existing:
        return existing
    result = bpy.data.materials.new(key)
    result.diffuse_color = color
    result.use_nodes = True
    result.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = color
    result.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.82
    return result


def part(obj, name, color, collection, parent=None):
    obj.name = name
    obj.data.materials.append(material(color))
    for linked in tuple(obj.users_collection):
        linked.objects.unlink(obj)
    collection.objects.link(obj)
    if parent:
        attach(obj, parent)
    return obj


def blender_point(point):
    """Convert Zoo/glTF's X-right, Y-up, Z-forward into Blender's Z-up space."""
    x, y, z = point
    return (x, z, y)


def box(name, location, scale, color, collection, parent=None, bevel=0.025):
    bpy.ops.mesh.primitive_cube_add(location=blender_point(location))
    obj = bpy.context.object
    obj.scale = tuple(value / 2 for value in blender_point(scale))
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel > 0:
        modifier = obj.modifiers.new("editable_edge_softening", "BEVEL")
        modifier.width = min(bevel, min(scale) * 0.2)
        modifier.segments = 2
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    return part(obj, name, color, collection, parent)


def cylinder(name, location, radius, depth, color, collection, parent=None, vertices=16, axis="Y"):
    bpy.ops.mesh.primitive_cylinder_add(vertices=vertices, radius=radius, depth=depth, location=blender_point(location))
    obj = bpy.context.object
    # Blender primitives are +Z-aligned, which corresponds to Zoo's +Y-up.
    if axis == "X": obj.rotation_euler[1] = math.pi / 2
    elif axis == "Z": obj.rotation_euler[0] = math.pi / 2
    elif axis != "Y": raise ValueError(f"unsupported cylinder axis: {axis}")
    return part(obj, name, color, collection, parent)


def cone(name, location, radius1, radius2, depth, color, collection, parent=None, vertices=16, axis="Y"):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius1, radius2=radius2, depth=depth, location=blender_point(location))
    obj = bpy.context.object
    if axis == "X": obj.rotation_euler[1] = math.pi / 2
    elif axis == "Z": obj.rotation_euler[0] = math.pi / 2
    elif axis != "Y": raise ValueError(f"unsupported cone axis: {axis}")
    return part(obj, name, color, collection, parent)


def sphere(name, location, scale, color, collection, parent=None, subdivisions=2):
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=subdivisions, radius=1, location=blender_point(location))
    obj = bpy.context.object
    obj.scale = blender_point(scale)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return part(obj, name, color, collection, parent)


def gable_roof(name, location, scale, color, collection, parent=None):
    """Create an exactly symmetric triangular-prism roof with a centered ridge."""
    width, height, depth = scale
    logical_vertices = [
        (-width/2, -height/2, -depth/2), (width/2, -height/2, -depth/2), (0, height/2, -depth/2),
        (-width/2, -height/2, depth/2), (width/2, -height/2, depth/2), (0, height/2, depth/2),
    ]
    vertices = [tuple(a + b for a, b in zip(blender_point(vertex), blender_point(location))) for vertex in logical_vertices]
    faces = [(0, 1, 2), (3, 5, 4), (0, 3, 4, 1), (0, 2, 5, 3), (1, 4, 5, 2)]
    mesh = bpy.data.meshes.new(f"{name}_mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    collection.objects.link(obj)
    return part(obj, name, color, collection, parent)


def root_node(name, collection):
    root = bpy.data.objects.new(name, None)
    collection.objects.link(root)
    return root


def attach(child, parent):
    """Parent a dependent part beneath a moving part while keeping its rest pose."""
    bpy.context.view_layer.update()
    world = child.matrix_world.copy()
    child.parent = parent
    child.matrix_parent_inverse = mathutils.Matrix.Identity(4)
    child.matrix_basis = parent.matrix_world.inverted() @ world
    return child


def joint(obj, end="top"):
    """Move a part's origin to its anatomical joint: the cap center of its upright geometry.

    Geometry stays where it was in the rest pose; only the rotation pivot changes, so
    keyframed rotations swing the part around the joint instead of its center.
    """
    vertices = [vertex.co for vertex in obj.data.vertices]
    pivot = mathutils.Vector((
        (min(v.x for v in vertices) + max(v.x for v in vertices)) / 2,
        (min(v.y for v in vertices) + max(v.y for v in vertices)) / 2,
        max(v.z for v in vertices) if end == "top" else min(v.z for v in vertices),
    ))
    obj.data.transform(mathutils.Matrix.Translation(-pivot))
    obj.matrix_basis = obj.matrix_basis @ mathutils.Matrix.Translation(pivot)
    return obj


def animate_rotation(obj, action_name, axis, amount, duration=1.0):
    axis = axis.lower()
    obj.rotation_mode = "XYZ"
    base = getattr(obj.rotation_euler, axis)
    setattr(obj.rotation_euler, axis, base)
    obj.keyframe_insert("rotation_euler", index="xyz".index(axis), frame=1)
    setattr(obj.rotation_euler, axis, base + amount)
    obj.keyframe_insert("rotation_euler", index="xyz".index(axis), frame=13)
    setattr(obj.rotation_euler, axis, base)
    obj.keyframe_insert("rotation_euler", index="xyz".index(axis), frame=25)
    action = obj.animation_data.action
    action.name = action_name
    bpy.context.scene.render.fps = 24


def animate_idle(root, moving_part=None, rotation_axis="Y", rotation_amount=.08):
    root.location.z = 0
    root.keyframe_insert("location", frame=1)
    root.location.z = .035
    root.keyframe_insert("location", frame=13)
    root.location.z = 0
    root.keyframe_insert("location", frame=25)
    root.animation_data.action.name = "idle"
    if moving_part is not None:
        animate_rotation(moving_part, "idle", rotation_axis, rotation_amount)


def make_human(asset, outfit):
    collection = bpy.data.collections.new(asset)
    bpy.context.scene.collection.children.link(collection)
    root = root_node("character_root", collection)
    head = sphere("head", (0, 1.58, 0), (0.17, 0.18, 0.16), "skin", collection, root, 3)
    sphere("nose", (0, 1.57, .155), (.045, .055, .04), "skin", collection, head, 1)
    for side in (-1, 1):
        sphere(f"eye_{side}", (side * .065, 1.63, .145), (.025, .025, .018), "black", collection, head, 1)
    box("torso", (0, 1.12, 0), (0.36, 0.52, 0.22), outfit, collection, root)
    for side in (-1, 1):
        arm = cylinder(f"arm_{'left' if side < 0 else 'right'}", (side * .25, 1.15, 0), .065, .48, outfit, collection, root)
        arm.rotation_euler[1] = side * .12
        joint(arm)
        attach(sphere(f"hand_{side}", (side * .27, .91, 0), (.075, .075, .075), "skin", collection, root, 1), arm)
        animate_rotation(arm, "walk", "X", side * -.5)
        leg = joint(cylinder(f"leg_{'left' if side < 0 else 'right'}", (side * .105, .52, 0), .075, .62, "dark_wood", collection, root))
        attach(box(f"shoe_{side}", (side * .105, .17, .055), (.16, .1, .27), "black", collection, root, .015), leg)
        animate_rotation(leg, "walk", "X", side * .45)
    attach(box("cap", (0, 1.76, 0), (.34, .08, .28), outfit, collection, root), head)
    if asset == "keeper":
        box("feed_satchel", (.25, 1.03, -.14), (.24, .3, .14), "crate", collection, root)
    elif asset == "janitor":
        cylinder("broom_handle", (.34, .75, 0), .025, 1.15, "crate", collection, root)
        box("broom_head", (.34, .18, 0), (.32, .1, .12), "food", collection, root, .01)
    elif asset == "mechanic":
        box("toolbox", (.3, .78, 0), (.32, .22, .18), "roof", collection, root)
        box("toolbox_handle", (.3, .93, 0), (.2, .08, .05), "dark_wood", collection, root, .01)
    else:
        box("backpack", (0, 1.17, -.17), (.3, .4, .14), "drink", collection, root)
    animate_idle(root, head, "Z", .12)
    return collection, ["idle", "walk"]


def make_quadruped(asset, body_color, profile):
    collection = bpy.data.collections.new(asset); bpy.context.scene.collection.children.link(collection)
    root = root_node("animal_root", collection)
    sphere("body", (0, .68, 0), profile["body"], body_color, collection, root, 3)
    head_scale = profile["head"]
    head_x = profile["head_x"]
    head_y = profile["head_y"]
    head = sphere("head", (head_x, head_y, 0), head_scale, body_color, collection, root, 3)
    sphere("eye_left", (head_x + head_scale[0] * .55, head_y + head_scale[1] * .25, -.13), (.035, .035, .025), "black", collection, head, 1)
    sphere("eye_right", (head_x + head_scale[0] * .55, head_y + head_scale[1] * .25, .13), (.035, .035, .025), "black", collection, head, 1)
    if profile.get("muzzle", True):
        sphere("muzzle", (head_x + head_scale[0] * .82, head_y - .035, 0), (head_scale[0] * .48, head_scale[1] * .42, head_scale[2] * .65), profile.get("muzzle_color", body_color), collection, head, 2)
        sphere("nose", (head_x + head_scale[0] * 1.25, head_y - .02, 0), (.055, .045, .065), "black", collection, head, 1)
    if profile.get("ears", True):
        for side in (-1, 1):
            sphere(f"ear_{side}", (head_x, head_y + head_scale[1] * .82, side * head_scale[2] * .72), (.07, .1, .055), body_color, collection, head, 2)
    leg_positions = ((-.38, -.22), (-.38, .22), (.38, -.22), (.38, .22))
    for index, (x, z) in enumerate(leg_positions):
        leg = joint(cylinder(f"leg_{index}", (x, .3, z), profile["leg_r"], .58, body_color, collection, root))
        attach(box(f"hoof_{index}", (x, .055, z + .025), (profile["leg_r"] * 2.2, .1, profile["leg_r"] * 2.8), profile.get("hoof_color", "black"), collection, root, .012), leg)
        animate_rotation(leg, "walk", "X", (.35 if index in (0, 3) else -.35))
    if profile.get("tail", True):
        tail = cylinder("tail", (-profile["body"][0] * .95, .72, 0), profile["leg_r"] * .55, profile["body"][0] * .55, body_color, collection, root, axis="X")
        animate_rotation(tail, "idle", "Z", .18)
    animate_idle(root, head, "Z", .08)
    return collection, ["idle", "walk"]


def make_animal(asset):
    if asset == "capybara":
        collection, clips = make_quadruped(asset, "capybara", {"body": (.56,.3,.3), "head_x":.56,"head_y":.68,"head":(.24,.2,.2),"leg_r":.075,"hoof_color":"capybara"})
        root = bpy.data.objects["animal_root"]
        for side in (-1, 1):
            attach(sphere(f"capybara_cheek_{side}", (.72, .65, side * .16), (.13, .11, .08), "capybara", collection, root, 2), bpy.data.objects["head"])
        return collection, clips
    if asset == "zebra":
        collection, clips = make_quadruped(asset, "zebra", {"body":(.54,.32,.25),"head_x":.54,"head_y":.94,"head":(.2,.28,.18),"leg_r":.055})
        root = bpy.data.objects["animal_root"]
        for index, x in enumerate((-.42,-.27,-.11,.06,.23,.39)):
            sphere(f"stripe_{index}", (x,.69,0), (.035,.315,.255), "black", collection, root, 2)
        box("mane", (.28, 1.05, 0), (.48, .09, .08), "black", collection, root, .015)
        return collection, clips
    if asset == "elephant":
        collection, clips = make_quadruped(asset, "elephant", {"body":(.7,.45,.42),"head_x":.65,"head_y":.82,"head":(.35,.35,.32),"leg_r":.12,"muzzle":False,"ears":False,"hoof_color":"elephant"})
        root = bpy.data.objects["animal_root"]
        head = bpy.data.objects["head"]
        trunk = cylinder("trunk", (.94,.5,0), .08,.58,"elephant",collection,root)
        trunk.rotation_euler[1] = math.pi / 6
        attach(joint(trunk), head)
        attach(sphere("ear_left", (.58,.91,-.31), (.24,.25,.045), "elephant", collection, root), head)
        attach(sphere("ear_right", (.58,.91,.31), (.24,.25,.045), "elephant", collection, root), head)
        for side in (-1, 1):
            tusk = cone(f"tusk_{side}", (.92, .67, side * .17), .055, .005, .32, "white", collection, root, axis="X")
            tusk.rotation_euler[1] += -.22
            attach(tusk, head)
        animate_rotation(trunk, "idle", "Y", .16)
        return collection, clips
    if asset == "giraffe":
        collection, clips = make_quadruped(asset, "giraffe", {"body":(.52,.32,.24),"head_x":.48,"head_y":1.75,"head":(.18,.2,.15),"leg_r":.055})
        root=bpy.data.objects["animal_root"]
        head=bpy.data.objects["head"]
        neck = joint(cylinder("neck",(.42,1.23,0),.11,.95,"giraffe",collection,root), "bottom")
        attach(head, neck)
        for side in (-1, 1):
            attach(cylinder(f"ossicone_{side}",(.48,2.0,side*.08),.025,.16,"giraffe",collection,root,12), head)
            attach(sphere(f"ossicone_tip_{side}",(.48,2.09,side*.08),(.04,.04,.04),"roof",collection,root,1), head)
        for index, (x,y,z) in enumerate(((-.28,.73,-.2),(-.1,.83,.19),(.12,.63,-.2),(.28,.82,.18),(.41,1.08,-.1),(.42,1.35,.1),(.43,1.58,-.08))):
            spot = sphere(f"spot_{index}",(x,y,z),(.085,.09,.025),"roof",collection,root,1)
            if index >= 4: attach(spot, neck)
        for index, y in enumerate((1.0,1.2,1.4,1.6)):
            attach(box(f"mane_{index}",(.31,y,-.105),(.1,.16,.035),"roof",collection,root,.008), neck)
        animate_rotation(neck, "idle", "X", .04)
        return collection, clips
    if asset == "flamingo":
        collection=bpy.data.collections.new(asset); bpy.context.scene.collection.children.link(collection); root=root_node("animal_root",collection)
        sphere("body",(0,.95,0),(.28,.38,.2),"pink",collection,root,3); head=sphere("head",(.2,1.85,0),(.14,.14,.12),"pink",collection,root,3)
        neck=joint(cylinder("neck",(.12,1.43,0),.055,.7,"pink",collection,root), "bottom"); attach(head, neck); attach(cone("beak",(.37,1.84,0),.075,.012,.25,"black",collection,root,axis="X"), head)
        for side in (-1, 1): sphere(f"eye_{side}",(.29,1.89,side*.085),(.022,.022,.014),"black",collection,head,1)
        for side in (-1,1):
            leg=joint(cylinder(f"leg_{side}",(side*.07,.44,0),.035,.82,"pink",collection,root))
            attach(box(f"foot_{side}",(side*.07,.035,.08),(.13,.045,.26),"pink",collection,root,.01), leg); animate_rotation(leg,"walk","X",side*.3)
        animate_idle(root, head, "Z", .1); animate_rotation(neck,"idle","X",.05); return collection,["idle","walk"]
    if asset == "penguin":
        collection=bpy.data.collections.new(asset); bpy.context.scene.collection.children.link(collection); root=root_node("animal_root",collection)
        sphere("body",(0,.55,0),(.28,.5,.23),"penguin",collection,root,3); sphere("belly",(0,.54,.22),(.18,.35,.035),"white",collection,root,2); head=sphere("head",(0,1.08,0),(.2,.2,.18),"penguin",collection,root,3)
        attach(cone("beak",(0,1.08,.24),.07,.005,.18,"food",collection,root,axis="Z"), head)
        for side in (-1,1): sphere(f"eye_{side}",(side*.07,1.13,.16),(.025,.025,.018),"white",collection,head,1); sphere(f"pupil_{side}",(side*.07,1.13,.178),(.012,.012,.008),"black",collection,head,1)
        for side in (-1,1):
            wing=joint(box(f"wing_{side}",(side*.27,.64,0),(.1,.38,.1),"penguin",collection,root)); animate_rotation(wing,"walk","Y",side*.35)
            box(f"foot_{side}",(side*.11,.08,.1),(.17,.07,.26),"food",collection,root,.015)
        animate_idle(root,head,"Z",.08); return collection,["idle","walk"]
    raise ValueError(asset)


def make_static(asset):
    collection=bpy.data.collections.new(asset); bpy.context.scene.collection.children.link(collection); root=root_node(f"{asset}_root",collection)
    if asset == "entrance":
        box("plinth",(0,.06,0),(1.72,.12,.94),"path",collection,root)
        for x in (-.56,.56):
            box(f"wing_{x}",(x,.49,0),(.5,.86,.68),"cream",collection,root)
            gable_roof(f"roof_{x}",(x,1.02,0),(.66,.28,.84),"roof",collection,root)
            box(f"window_{x}",(x,.57,.352),(.22,.3,.035),"drink",collection,root,.01)
        box("tower",(0,.62,0),(.58,1.12,.74),"cream",collection,root)
        cone("tower_roof",(0,1.28,0),.43,.09,.34,"roof",collection,root)
        box("door",(0,.39,.39),(.28,.54,.06),"teal",collection,root)
        box("zoo_sign",(0,.88,.405),(.64,.18,.05),"yellow",collection,root)
        for x in (-.23,.23): box(f"column_{x}",(x,.47,.42),(.08,.72,.08),"trim",collection,root,.012)
    elif asset == "animal_care_depot":
        box("plinth",(0,.06,0),(1.3,.12,1.02),"path",collection,root); box("shell",(0,.49,0),(1.18,.86,.9),"cream",collection,root); box("roof",(0,.98,0),(1.32,.12,1.04),"teal",collection,root); box("garage_door",(0,.39,.475),(.72,.56,.05),"teal",collection,root); box("sign",(0,.84,.495),(.62,.17,.05),"yellow",collection,root); cylinder("roof_vent",(-.28,1.16,-.08),.075,.22,"elephant",collection,root)
        for index, y in enumerate((.18,.31,.44,.57)):
            box(f"garage_slat_{index}",(0,y,.508),(.66,.025,.018),"trim",collection,root,.004)
        box("side_window",(.39,.64,.458),(.24,.2,.025),"drink",collection,root,.008)
    elif asset in ("food_stand","drink_stand"):
        accent="food" if asset=="food_stand" else "drink"
        box("counter",(0,.38,0),(.82,.7,.62),accent,collection,root)
        box("service_window",(0,.47,.322),(.54,.28,.025),"dark_wood",collection,root,.008)
        box("serving_counter",(0,.31,.38),(.68,.14,.16),"cream",collection,root)
        for x in (-.42,.42): cylinder(f"awning_post_{x}",(x,.66,.28),.025,.54,"trim",collection,root,12)
        for index, x in enumerate((-.4,-.2,0,.2,.4)):
            box(f"awning_stripe_{index}",(x,.84,.08),(.205,.13,.78),accent if index % 2 == 0 else "white",collection,root,.012)
        box("menu_sign",(0,1.02,.03),(.5,.23,.08),accent,collection,root)
        if asset == "food_stand":
            cylinder("burger_bun_bottom",(0,1.08,.085),.11,.045,"food",collection,root,16)
            cylinder("burger_patty",(0,1.13,.085),.1,.035,"roof",collection,root,16)
            cylinder("burger_bun_top",(0,1.18,.085),.11,.05,"food",collection,root,16)
        else:
            cylinder("drink_cup",(0,1.12,.085),.075,.17,"white",collection,root,16)
            cylinder("drink_straw",(.035,1.25,.085),.012,.14,"roof",collection,root,10)
    elif asset == "fence_segment":
        for y in (.27,.51): box(f"rail_{y}",(0,y,0),(1,.07,.075),"wood",collection,root)
        for x in (-.5,.5): cylinder(f"post_{x}",(x,.36,0),.055,.72,"dark_wood",collection,root); cylinder(f"cap_{x}",(x,.745,0),.075,.05,"path",collection,root)
    elif asset in ("grass_tile","path_tile","habitat_marker"):
        color={"grass_tile":"leaf","path_tile":"path","habitat_marker":"wood"}[asset]; box("tile",(0,-.06,0),(1,.12,1),color,collection,root)
        if asset == "habitat_marker": box("marker",(0,.02,0),(.55,.025,.55),"yellow",collection,root)
    elif asset == "feed_crate":
        box("crate",(0,.24,0),(.5,.48,.5),"crate",collection,root)
        for x in (-.22,.22): box(f"brace_{x}",(x,.24,.255),(.06,.44,.03),"dark_wood",collection,root,.008)
        for index, y in enumerate((.08,.24,.4)): box(f"front_slat_{index}",(0,y,.268),(.45,.055,.035),"dark_wood",collection,root,.008)
        box("feed_label",(0,.25,.29),(.18,.12,.025),"yellow",collection,root,.006)
    elif asset == "litter":
        box("paper",(-.1,.02,0),(.26,.035,.16),"crate",collection,root); cylinder("cup",(.12,.06,.06),.055,.11,"food",collection,root,12)
    else: raise ValueError(asset)
    return collection, []


ASSET_IDS = ["entrance", "animal_care_depot", "food_stand", "drink_stand", "fence_segment", "grass_tile", "path_tile", "habitat_marker", "feed_crate", "litter", "capybara", "flamingo", "zebra", "giraffe", "elephant", "penguin", "guest", "keeper", "janitor", "mechanic"]

TRIANGLE_BUDGETS = {
    "entrance": 2500, "animal_care_depot": 2000, "food_stand": 2500, "drink_stand": 2500,
    "fence_segment": 1000, "grass_tile": 400, "path_tile": 400, "habitat_marker": 500,
    "feed_crate": 1000, "litter": 500,
    "capybara": 3000, "flamingo": 2500, "zebra": 3500, "giraffe": 3500,
    "elephant": 3500, "penguin": 2500,
    "guest": 2000, "keeper": 2000, "janitor": 2000, "mechanic": 2000,
}


def build_asset(asset):
    bpy.ops.object.select_all(action="SELECT"); bpy.ops.object.delete(use_global=False)
    for collection in list(bpy.data.collections):
        if collection.name != "Collection": bpy.data.collections.remove(collection)
    if asset in {"capybara","flamingo","zebra","giraffe","elephant","penguin"}: collection, clips=make_animal(asset)
    elif asset in {"guest","keeper","janitor","mechanic"}: collection, clips=make_human(asset, {"guest":"roof","keeper":"keeper","janitor":"janitor","mechanic":"mechanic"}[asset])
    else: collection, clips=make_static(asset)
    bpy.context.view_layer.update()
    return collection, clips


def triangle_count(collection):
    return sum(sum(max(0, len(poly.vertices)-2) for poly in obj.data.polygons) for obj in collection.all_objects if obj.type == "MESH")


def render_preview(asset, collection):
    """Render a deterministic three-quarter inspection view for the catalog."""
    PREVIEWS.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.camera_add(location=(3.2, 4.8, 2.8))
    camera = bpy.context.object
    camera.name = "preview_camera"
    camera.rotation_euler = ((mathutils.Vector((0, 0, .72)) - camera.location).to_track_quat("-Z", "Y").to_euler())
    bpy.context.scene.camera = camera
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_WORKBENCH"
    scene.display.shading.light = "STUDIO"
    scene.display.shading.studio_light = "paint.sl"
    scene.display.shading.show_shadows = True
    scene.display.shading.show_cavity = True
    scene.display.shading.cavity_type = "WORLD"
    scene.display.shading.color_type = "MATERIAL"
    scene.render.resolution_x = 420
    scene.render.resolution_y = 420
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = False
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(PREVIEWS / f"{asset}.png")
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(camera, do_unlink=True)


def normalized_animations(document):
    """Merge Blender's per-object actions into complete named glTF clips."""
    animations = document.get("animations", [])
    if not any(clip.get("name", "").startswith(("idle", "walk")) for clip in animations):
        return document

    def merged(prefix):
        matching = [clip for clip in animations if clip.get("name", "").startswith(prefix)]
        if not matching:
            return None
        result = {"name": prefix, "samplers": [], "channels": []}
        for clip in matching:
            sampler_offset = len(result["samplers"])
            result["samplers"].extend(clip.get("samplers", []))
            for channel in clip.get("channels", []):
                copied = dict(channel)
                copied["sampler"] = channel["sampler"] + sampler_offset
                result["channels"].append(copied)
        return result

    document["animations"] = [clip for prefix in ("idle", "walk") if (clip := merged(prefix)) is not None]
    return document


def normalize_animation_export(path):
    if path.suffix == ".gltf":
        path.write_text(json.dumps(normalized_animations(json.loads(path.read_text())), separators=(",", ":")))
        return
    data = path.read_bytes()
    magic, version, _length = struct.unpack_from("<4sII", data)
    json_length, chunk_type = struct.unpack_from("<I4s", data, 12)
    assert magic == b"glTF" and version == 2 and chunk_type == b"JSON"
    document = normalized_animations(json.loads(data[20:20 + json_length].decode("utf8")))
    encoded = json.dumps(document, separators=(",", ":")).encode("utf8")
    encoded += b" " * ((4 - len(encoded) % 4) % 4)
    remainder = data[20 + json_length:]
    path.write_bytes(struct.pack("<4sII", b"glTF", 2, 20 + len(encoded) + len(remainder)) + struct.pack("<I4s", len(encoded), b"JSON") + encoded + remainder)


def export(asset, stage):
    collection, clips=build_asset(asset)
    destination = DEVELOPMENT / asset if stage == "development" else PRODUCTION
    destination.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in collection.all_objects: obj.select_set(True)
    bpy.context.view_layer.objects.active = next(obj for obj in collection.all_objects if obj.type == "MESH")
    if stage == "development":
        bpy.ops.wm.save_as_mainfile(filepath=str(destination / f"{asset}.blend"), check_existing=False)
        bpy.ops.export_scene.gltf(filepath=str(destination / f"{asset}.gltf"), export_format="GLTF_SEPARATE", use_selection=True, export_animations=True, export_materials="EXPORT")
        render_preview(asset, collection)
        path=destination / f"{asset}.gltf"
    else:
        bpy.ops.export_scene.gltf(filepath=str(destination / f"{asset}.glb"), export_format="GLB", use_selection=True, export_animations=True, export_materials="EXPORT")
        path=destination / f"{asset}.glb"
    normalize_animation_export(path)
    return {"id": asset, "stage": stage, "path": path.relative_to(ROOT).as_posix(), "triangles": triangle_count(collection), "triangleBudget": TRIANGLE_BUDGETS[asset], "materials": len({slot.material.name for obj in collection.all_objects if obj.type == "MESH" for slot in obj.material_slots if slot.material}), "animations": clips}


def validate_gltf(path):
    document=json.loads(path.read_text()); directory=path.parent
    for buffer in document.get("buffers", []):
        uri=buffer.get("uri"); assert uri and (directory / uri).is_file(), f"missing buffer: {path}/{uri}"
    assert document.get("nodes"), f"no nodes: {path}"
    return document


def validate_glb(path):
    data=path.read_bytes(); magic, version, length=struct.unpack_from("<4sII",data); assert magic == b"glTF" and version == 2 and length == len(data), f"invalid GLB: {path}"
    json_length, chunk_type=struct.unpack_from("<I4s",data,12); assert chunk_type == b"JSON", f"missing JSON chunk: {path}"
    return json.loads(data[20:20 + json_length].decode("utf8"))


def assert_finite_positions(document, path):
    """Read every exported POSITION float, rather than trusting accessor bounds."""
    buffers = document_buffers(document, path)
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            accessor = document["accessors"][primitive["attributes"]["POSITION"]]
            assert accessor["componentType"] == 5126 and accessor["type"] == "VEC3", f"unsupported positions: {path}"
            values = read_accessor_floats(document, buffers, accessor)
            assert all(math.isfinite(value) for value in values), f"non-finite positions: {path}"


def document_buffers(document, path):
    if path.suffix == ".gltf":
        return [(path.parent / buffer["uri"]).read_bytes() for buffer in document.get("buffers", [])]
    data = path.read_bytes()
    json_length = struct.unpack_from("<I", data, 12)[0]
    offset = 20 + json_length
    if offset >= len(data): return []
    binary_length, chunk_type = struct.unpack_from("<I4s", data, offset)
    assert chunk_type == b"BIN\x00", f"missing binary chunk: {path}"
    return [data[offset + 8:offset + 8 + binary_length]]


def read_accessor_floats(document, buffers, accessor):
    components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
    assert accessor["componentType"] == 5126
    view = document["bufferViews"][accessor["bufferView"]]
    stride = view.get("byteStride", components * 4)
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    buffer = buffers[view.get("buffer", 0)]
    values = []
    for index in range(accessor["count"]):
        values.extend(struct.unpack_from(f"<{components}f", buffer, start + index * stride))
    return values


def assert_animation_motion(document, path):
    buffers = document_buffers(document, path)
    for animation in document.get("animations", []):
        moving = False
        for sampler in animation.get("samplers", []):
            accessor = document["accessors"][sampler["output"]]
            values = read_accessor_floats(document, buffers, accessor)
            components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
            keys = [values[index:index + components] for index in range(0, len(values), components)]
            if keys and any(any(abs(a - b) > 1e-5 for a, b in zip(keys[0], key)) for key in keys[1:]):
                moving = True
                break
        assert moving, f"animation has no motion: {path}:{animation.get('name')}"


def document_triangle_count(document):
    total = 0
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            accessor_index = primitive.get("indices", primitive["attributes"]["POSITION"])
            total += document["accessors"][accessor_index]["count"] // 3
    return total


def importable(path):
    bpy.ops.object.select_all(action="SELECT"); bpy.ops.object.delete(use_global=False)
    bpy.ops.import_scene.gltf(filepath=str(path))
    assert any(obj.type == "MESH" for obj in bpy.context.scene.objects), f"failed to load mesh: {path}"


def validate_outputs():
    catalog = {entry["id"]: entry for entry in json.loads(MANIFEST.read_text())["assets"]}
    for asset in ASSET_IDS:
        development=DEVELOPMENT / asset / f"{asset}.gltf"; production=PRODUCTION / f"{asset}.glb"
        dev_doc=validate_gltf(development); prod_doc=validate_glb(production)
        assert_finite_positions(dev_doc, development); assert_finite_positions(prod_doc, production)
        dev_triangles = document_triangle_count(dev_doc); prod_triangles = document_triangle_count(prod_doc)
        assert dev_triangles == prod_triangles == catalog[asset]["triangles"], f"triangle drift: {asset}"
        assert dev_triangles <= TRIANGLE_BUDGETS[asset], f"triangle budget exceeded: {asset}"
        expected = ["idle", "walk"] if asset in {"capybara","flamingo","zebra","giraffe","elephant","penguin","guest","keeper","janitor","mechanic"} else []
        assert [clip.get("name") for clip in dev_doc.get("animations", [])] == expected, f"development clips: {development}"
        assert [clip.get("name") for clip in prod_doc.get("animations", [])] == expected, f"production clips: {production}"
        if expected:
            for document, path in ((dev_doc, development), (prod_doc, production)):
                channels = {clip["name"]: len(clip.get("channels", [])) for clip in document["animations"]}
                assert channels["idle"] >= 2 and channels["walk"] >= 2, f"incomplete animation channels: {path}"
                assert_animation_motion(document, path)
        importable(development); importable(production)
    print(f"validated {len(ASSET_IDS)} development packages and production GLBs")


def main():
    options=args()
    if options.validate: validate_outputs(); return
    rows=[]
    for stage in (("development","production") if options.stage == "all" else (options.stage,)):
        for asset in ASSET_IDS: rows.append(export(asset,stage))
    if options.stage in ("all","development"):
        # The catalog is generated from exact geometry observations, not target budgets.
        grouped={row["id"]: row for row in rows if row["stage"] == "development"}
        MANIFEST.parent.mkdir(parents=True,exist_ok=True)
        MANIFEST.write_text(json.dumps({"schemaVersion":1,"artDirection":"cheerful low-poly isometric","units":"meters; +Y up; one tile is one meter","assets":[{**grouped[asset],"production":"assets/3d/production/%s.glb" % asset} for asset in ASSET_IDS]},indent=2)+"\n")
    print(json.dumps(rows, indent=2))

if __name__ == "__main__": main()
