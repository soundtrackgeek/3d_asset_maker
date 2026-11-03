"""Dev helper: report system-RAM growth as each TRELLIS.2 checkpoint loads. Aborts above 14 GB.
Run with the runtime venv python: ram-profile.py <trellis_repo>"""
import gc, os, sys, json, ctypes
from ctypes import wintypes

sys.path.insert(0, sys.argv[1])
os.environ["OPENCV_IO_ENABLE_OPENEXR"] = "1"


class PMC(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("pf", wintypes.DWORD)] + [(n, ctypes.c_size_t) for n in
                ("a", "ws", "b", "c", "d", "e", "pagefile", "peakpf", "priv")]


def priv():
    p = PMC(); p.cb = ctypes.sizeof(p)
    k = ctypes.windll.kernel32; k.GetCurrentProcess.restype = wintypes.HANDLE
    ctypes.windll.psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(PMC), wintypes.DWORD]
    ctypes.windll.psapi.GetProcessMemoryInfo(k.GetCurrentProcess(), ctypes.byref(p), p.cb)
    return p.priv >> 20


import torch
from trellis2 import models
sys.path.insert(0, "crates/core/python"); import worker; worker.install_lean_loader()
from huggingface_hub import hf_hub_download

cfg = json.load(open(hf_hub_download("microsoft/TRELLIS.2-4B", "pipeline.json")))["args"]["models"]
print("start", priv(), flush=True)
keep = []
for k, v in cfg.items():
    if "512" in k and "--all" not in sys.argv:
        continue
    try:
        m = models.from_pretrained(f"microsoft/TRELLIS.2-4B/{v}")
    except Exception:
        m = models.from_pretrained(v)
    gc.collect()
    nbytes = sum(p.numel() * p.element_size() for p in m.parameters()) >> 20
    print(f"{k}: params={nbytes} MiB dtypes={ {str(p.dtype) for p in m.parameters()} } private={priv()} MiB", flush=True)
    keep.append(m)
    if priv() > 17000:
        print("ABORT: over 14 GB"); break

del keep; gc.collect()
print("-- conditioners (models freed first, to isolate them)", flush=True)
from trellis2.pipelines import rembg
from trellis2.modules import image_feature_extractor
a = json.load(open(hf_hub_download("microsoft/TRELLIS.2-4B", "pipeline.json")))["args"]
m1 = getattr(image_feature_extractor, a["image_cond_model"]["name"])(**a["image_cond_model"]["args"]); gc.collect()
print("dino", priv(), flush=True)
m2 = getattr(rembg, a["rembg_model"]["name"])(**a["rembg_model"]["args"]); gc.collect()
print("rembg", priv(), flush=True)
