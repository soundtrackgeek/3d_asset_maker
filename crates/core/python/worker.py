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


def install_lean_loader():
    """Load TRELLIS.2 checkpoints without the transient fp32 copy.

    Upstream builds each model with real (random-initialised) weights and then copies the checkpoint over,
    which on Windows leaves ~2x the model size stuck in the process heap (about 19 GB for the cascade
    pipeline). Build on the meta device and adopt the checkpoint tensors instead (one copy), keeping each
    parameter's intended dtype. Models with buffers or missing keys fall back to the upstream path.
    """
    import json
    import torch
    from safetensors.torch import load_file
    from trellis2 import models

    orig = models.from_pretrained

    def lean(path, **kwargs):
        import os

        is_local = os.path.exists(f"{path}.json") and os.path.exists(f"{path}.safetensors")
        if is_local:
            config_file, model_file = f"{path}.json", f"{path}.safetensors"
        else:
            from huggingface_hub import hf_hub_download

            parts = path.split("/")
            repo_id, name = f"{parts[0]}/{parts[1]}", "/".join(parts[2:])
            config_file = hf_hub_download(repo_id, f"{name}.json")
            model_file = hf_hub_download(repo_id, f"{name}.safetensors")
        try:
            config = json.load(open(config_file))
            with torch.device("meta"):
                model = models.__getattr__(config["name"])(**config["args"], **kwargs)
            if any(True for _ in model.buffers()):
                raise RuntimeError("has buffers")
            if any(isinstance(v, torch.Tensor) for mod in model.modules() for v in vars(mod).values()
                   if not isinstance(v, torch.nn.Parameter)):
                raise RuntimeError("has plain tensor attributes")  # e.g. rope tables built in __init__
            want = {k: p.dtype for k, p in model.named_parameters()}
            sd = load_file(model_file)
            if set(want) - set(sd):
                raise RuntimeError("checkpoint is missing parameters")
            sd = {k: (v.to(want[k]) if k in want else v) for k, v in sd.items()}
            model.load_state_dict(sd, strict=False, assign=True)
            return model
        except Exception as e:  # noqa: BLE001
            print("lean loader fallback for %s: %s" % (path, e), file=sys.stderr, flush=True)
            try:
                # Build and fill the model on the GPU (its init temporaries never touch the CPU heap), then
                # park the finished weights in system RAM, which is where the low-VRAM pipeline expects them.
                config = json.load(open(config_file))
                with torch.device("cuda"):
                    model = models.__getattr__(config["name"])(**config["args"], **kwargs)
                sd = load_file(model_file, device="cuda")
                model.load_state_dict(sd, strict=False)
                del sd
                model.cpu()
                torch.cuda.empty_cache()
                return model
            except Exception as e2:  # noqa: BLE001
                print("gpu-staged load failed for %s: %s" % (path, e2), file=sys.stderr, flush=True)
                torch.cuda.empty_cache()
            model = orig(path, **kwargs)
            with torch.no_grad():  # compact: frees the fragmented fp32 init blocks (~2 GB for a DiT)
                for p in model.parameters():
                    p.data = p.data.clone()
            import gc

            gc.collect()
            return model

    models.from_pretrained = lean


class LazyModels(dict):
    """pipeline.models, where the big flow models are read from disk on first use and can be evicted.

    The stock pipeline keeps every checkpoint in system RAM at once (~17 GB for the cascade pipeline). The
    stages run strictly one after another, so each flow model only has to exist while its stage runs.
    """

    def __init__(self, loaded, lazy_paths):
        super().__init__(loaded)
        self._lazy = dict(lazy_paths)

    def __contains__(self, k):
        return super().__contains__(k) or k in self._lazy

    def __getitem__(self, k):
        if not super().__contains__(k):
            from trellis2 import models

            path = self._lazy[k]
            try:
                m = models.from_pretrained(path[0])
            except Exception:  # noqa: BLE001
                m = models.from_pretrained(path[1])
            m.eval()
            super().__setitem__(k, m)
        return super().__getitem__(k)

    def evict(self, *names):
        for n in names:
            if n in self._lazy:
                super().pop(n, None)


PEAK_RAM = [0]  # MiB, highest private memory seen by the watchdog


def start_ram_watchdog():
    """Abort cleanly if this process's private memory passes T2_MAX_RAM_MB (protects the host PC)."""
    import threading

    limit = int(os.environ.get("T2_MAX_RAM_MB", "0"))
    if limit <= 0 or os.name != "nt":
        return
    import ctypes
    from ctypes import wintypes

    class PMC(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (n, ctypes.c_size_t)
            for n in ("PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage",
                      "QuotaPeakNonPagedPoolUsage", "QuotaNonPagedPoolUsage", "PagefileUsage",
                      "PeakPagefileUsage", "PrivateUsage")
        ]

    k32 = ctypes.windll.kernel32
    psapi = ctypes.windll.psapi
    k32.GetCurrentProcess.restype = wintypes.HANDLE
    psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(PMC), wintypes.DWORD]
    me = k32.GetCurrentProcess()

    def loop():
        pmc = PMC()
        pmc.cb = ctypes.sizeof(pmc)
        last = [0]
        while True:
            if psapi.GetProcessMemoryInfo(me, ctypes.byref(pmc), pmc.cb):
                used = pmc.WorkingSetSize >> 20  # physical RAM actually resident (what starves the PC)
                PEAK_RAM[0] = max(PEAK_RAM[0], used)
                priv = pmc.PrivateUsage >> 20
                if abs(used - last[0]) >= 1024:
                    last[0] = used
                    print("ram: working set %d MiB, private %d MiB, cuda %s" % (
                        used, priv, os.environ.get("T2_STAGE", "?")), file=sys.stderr, flush=True)
                if used > limit:
                    emit(event="error", message="Aborted: worker used %d MiB of system RAM (limit %d MiB, "
                         "set T2_MAX_RAM_MB to change). Use the 'draft' preset or close other apps." % (used, limit))
                    os._exit(3)
            time.sleep(0.25)

    threading.Thread(target=loop, daemon=True).start()


def main():
    start_ram_watchdog()
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

    # TRELLIS.2 asks for the gated briaai/RMBG-2.0 background remover. It is a BiRefNet, and the
    # open ZhengPeng7/BiRefNet (MIT) is the wrapper's own default, so fall back instead of failing
    # when the Hugging Face account has not been granted RMBG-2.0 access.
    from trellis2.pipelines import rembg as _rembg

    _cls = getattr(_rembg, "BiRefNet")
    _cls = getattr(_cls, "BiRefNet", _cls)  # tolerate module-vs-class export
    _orig_init = _cls.__init__

    def _init_with_fallback(self, model_name="ZhengPeng7/BiRefNet", *a, **k):
        try:
            _orig_init(self, model_name, *a, **k)
        except OSError as e:
            if model_name == "ZhengPeng7/BiRefNet" or not ("gated" in str(e).lower() or "403" in str(e)):
                raise
            progress("RMBG-2.0 not accessible on Hugging Face; using open BiRefNet for background removal", 0.05)
            _orig_init(self, "ZhengPeng7/BiRefNet", *a, **k)

    _cls.__init__ = _init_with_fallback

    progress("loading model weights", 0.05)
    # The stock pipeline loads every checkpoint (~13 GB of bf16 DiTs) into system RAM. Only load the
    # flow models this resolution actually uses; 'model_names_to_load' is honoured by the pipeline base.
    # Decoders (small) load up front; the 1.3B flow models are loaded per stage and evicted afterwards
    # (see LazyModels), so system RAM holds at most one or two of them instead of all four.
    _flow = ["sparse_structure_flow_model"] + {
        "512": ["shape_slat_flow_model_512", "tex_slat_flow_model_512"],
        "1024": ["shape_slat_flow_model_1024", "tex_slat_flow_model_1024"],
    }.get(o["resolution"], ["shape_slat_flow_model_512", "shape_slat_flow_model_1024", "tex_slat_flow_model_1024"])
    lazy = bool(o.get("low_vram", True))
    Trellis2ImageTo3DPipeline.model_names_to_load = ["sparse_structure_decoder", "shape_slat_decoder", "tex_slat_decoder"] + (
        [] if lazy else _flow
    )
    install_lean_loader()
    pipe = Trellis2ImageTo3DPipeline.from_pretrained(o["model_id"])
    if lazy:
        specs = pipe._pretrained_args["models"]
        pipe.models = LazyModels(pipe.models, {k: (f"{o['model_id']}/{specs[k]}", specs[k]) for k in _flow})
    # Low-VRAM: keep sub-models on CPU and stream them to the GPU per stage if the
    # installed TRELLIS.2 revision supports it; otherwise fall back to full .cuda().
    if o.get("low_vram") and hasattr(pipe, "low_vram"):
        pipe.low_vram = True
    pipe.cuda()

    # Windows' CUDA allocator keeps freed blocks reserved, and anything reserved beyond physical VRAM silently
    # spills into shared system memory (slow, and a source of native crashes). Release cached blocks after
    # every pipeline stage so the peak stays close to what is actually live.
    import gc

    def _release_after(name, evict=()):
        fn = getattr(pipe, name, None)
        if fn is None:
            return

        def wrapped(*a, **k):
            os.environ["T2_STAGE"] = name
            gc.collect()  # frees models evicted by the previous stage (their argument refs are gone by now)
            try:
                return fn(*a, **k)
            finally:
                if isinstance(pipe.models, LazyModels):
                    pipe.models.evict(*evict)
                gc.collect()
                torch.cuda.empty_cache()

        setattr(pipe, name, wrapped)

    _shape = ("shape_slat_flow_model_512", "shape_slat_flow_model_1024")
    _tex = ("tex_slat_flow_model_512", "tex_slat_flow_model_1024")
    for _n, _ev in (("get_cond", ()), ("sample_sparse_structure", ("sparse_structure_flow_model",)),
                    ("sample_shape_slat", _shape), ("sample_shape_slat_cascade", _shape),
                    ("decode_shape_slat", ()), ("sample_tex_slat", _tex), ("decode_tex_slat", ())):
        _release_after(_n, _ev)
    torch.cuda.reset_peak_memory_stats()

    image = Image.open(req["image"])
    tries = max(1, int(o.get("seed_tries") or 1))
    base_seed = int(o["seed"])

    def make_kwargs(seed):
        kw = dict(
            seed=seed,
            preprocess_image=bool(o["preprocess_image"]),
            pipeline_type=o["resolution"],
            max_num_tokens=int(o["max_num_tokens"]),
            sparse_structure_sampler_params=sampler(o.get("sparse_structure")),
            shape_slat_sampler_params=sampler(o.get("shape")),
            tex_slat_sampler_params=sampler(o.get("texture")),
        )
        # Tolerate API drift between TRELLIS.2 revisions.
        accepted = set(inspect.signature(pipe.run).parameters)
        return {k: v for k, v in kw.items() if k in accepted and v not in ({},)}

    results = []
    for i in range(tries):
        seed = base_seed + i
        span = 0.7 / tries
        progress("seed %d (%d/%d): generating" % (seed, i + 1, tries) if tries > 1
                 else "generating (sparse structure -> shape -> texture)", 0.2 + span * i)
        t0 = time.time()
        try:
            mesh = pipe.run(image, **make_kwargs(seed))[0]
        except torch.cuda.OutOfMemoryError:
            emit(
                event="error",
                message="CUDA out of memory. On a 16 GB card use the 'draft' preset, "
                "lower max_num_tokens, and close other GPU apps.",
            )
            sys.exit(2)
        progress(
            "generation finished in %.0fs (peak VRAM: %d MiB live, %d MiB reserved)"
            % (time.time() - t0, torch.cuda.max_memory_allocated() / 2**20, torch.cuda.max_memory_reserved() / 2**20),
            0.2 + span * (i + 0.6),
        )
        emit(
            event="stat",
            peak_vram_mb=int(torch.cuda.max_memory_reserved() / 2**20),
            peak_ram_mb=PEAK_RAM[0],
            peak_alloc_mb=int(torch.cuda.max_memory_allocated() / 2**20),
            total_vram_mb=int(torch.cuda.get_device_properties(0).total_memory / 2**20),
        )

        mesh.simplify(16777216)  # nvdiffrast limit
        progress("baking GLB (decimate + remesh + textures)", 0.2 + span * (i + 0.8))
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
        name = "model.glb" if tries == 1 else "model_seed%d.glb" % seed
        glb_path = os.path.join(out_dir, name)
        glb.export(glb_path, extension_webp=bool(o.get("webp_textures")))
        del mesh, glb
        gc.collect()
        torch.cuda.empty_cache()
        results.append((seed, name))
        if tries > 1:
            emit(event="candidate", seed=seed, file=name)

    # No reliable automatic quality metric exists for these artifacts, so the first seed becomes model.glb and
    # every candidate is kept for the user (or agent) to pick; `t2 jobs use <id> <seed>` promotes one.
    glb_path = os.path.join(out_dir, "model.glb")
    if tries > 1:
        import shutil

        shutil.copyfile(os.path.join(out_dir, results[0][1]), glb_path)
        for _seed, _name in results:
            emit(event="output", file=_name)
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
