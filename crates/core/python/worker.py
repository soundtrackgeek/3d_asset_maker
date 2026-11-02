"""TRELLIS.2 worker: image -> GLB (+ OBJ/PLY).

Protocol: reads a request.json path from argv[1]; prints progress as lines
starting with "@@" followed by JSON. Everything else (library noise) is ignored
by the host, and stderr goes to worker.log.
"""
import inspect
import json
import os
import sys
import time
import traceback


def emit(**kw):
    print("@@" + json.dumps(kw), flush=True)


def progress(stage, pct):
    emit(event="progress", stage=stage, pct=round(pct, 3))


def sampler(d):
    return {k: v for k, v in (d or {}).items() if v is not None}


def main():
    req = json.load(open(sys.argv[1], encoding="utf-8"))
    o = req["options"]
    out_dir = req["out_dir"]
    os.makedirs(out_dir, exist_ok=True)
    sys.path.insert(0, req["trellis_repo"])
    os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")
    os.environ["OPENCV_IO_ENABLE_OPENEXR"] = "1"

    progress("importing libraries", 0.02)
    import torch
    from PIL import Image
    from trellis2.pipelines import Trellis2ImageTo3DPipeline
    import o_voxel

    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is not available to PyTorch. Check the NVIDIA driver and the cu128 torch install.")

    progress("loading model weights", 0.05)
    pipe = Trellis2ImageTo3DPipeline.from_pretrained(o["model_id"])
    # Low-VRAM: keep sub-models on CPU and stream them to the GPU per stage if the
    # installed TRELLIS.2 revision supports it; otherwise fall back to full .cuda().
    if o.get("low_vram") and hasattr(pipe, "low_vram"):
        pipe.low_vram = True
    pipe.cuda()

    image = Image.open(req["image"])
    progress("generating (sparse structure -> shape -> texture)", 0.2)

    kwargs = dict(
        seed=int(o["seed"]),
        preprocess_image=bool(o["preprocess_image"]),
        pipeline_type=o["resolution"],
        max_num_tokens=int(o["max_num_tokens"]),
        sparse_structure_sampler_params=sampler(o.get("sparse_structure")),
        shape_slat_sampler_params=sampler(o.get("shape")),
        tex_slat_sampler_params=sampler(o.get("texture")),
    )
    # Tolerate API drift between TRELLIS.2 revisions.
    accepted = set(inspect.signature(pipe.run).parameters)
    kwargs = {k: v for k, v in kwargs.items() if k in accepted and v not in ({},)}

    t0 = time.time()
    try:
        mesh = pipe.run(image, **kwargs)[0]
    except torch.cuda.OutOfMemoryError:
        emit(
            event="error",
            message="CUDA out of memory. On a 16 GB card use the 'balanced' or 'draft' preset, "
            "lower max_num_tokens, and close other GPU apps.",
        )
        sys.exit(2)
    progress("generation finished in %.0fs" % (time.time() - t0), 0.75)

    mesh.simplify(16777216)  # nvdiffrast limit
    progress("baking GLB (decimate + remesh + textures)", 0.8)
    glb = o_voxel.postprocess.to_glb(
        vertices=mesh.vertices,
        faces=mesh.faces,
        attr_volume=mesh.attrs,
        coords=mesh.coords,
        attr_layout=mesh.layout,
        voxel_size=mesh.voxel_size,
        aabb=[[-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]],
        decimation_target=int(o["decimation_target"]),
        texture_size=int(o["texture_size"]),
        remesh=bool(o["remesh"]),
        remesh_band=1,
        remesh_project=0,
        verbose=True,
    )
    glb_path = os.path.join(out_dir, "model.glb")
    glb.export(glb_path, extension_webp=bool(o.get("webp_textures")))
    emit(event="output", file="model.glb")

    extra = [f for f in o["formats"] if f != "glb"]
    if extra:
        progress("converting extra formats", 0.95)
        import trimesh

        scene = trimesh.load(glb_path, force="mesh")
        for fmt in extra:
            name = "model." + fmt
            scene.export(os.path.join(out_dir, name))
            emit(event="output", file=name)

    # keep a copy of the (possibly background-removed) input for reference
    progress("done", 1.0)
    emit(event="done")


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as e:  # noqa: BLE001
        traceback.print_exc()
        emit(event="error", message="%s: %s" % (type(e).__name__, e))
        sys.exit(1)
