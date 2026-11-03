"""Dev helper: where does one DiT's RAM go? Prints private MiB after each step of loading."""
import gc, os, sys, json
sys.argv = [sys.argv[0], sys.argv[1]]
exec(open(os.path.join(os.path.dirname(__file__), "ram-profile.py")).read().split("import torch")[0])
import torch
from trellis2 import models
from safetensors.torch import load_file
from huggingface_hub import hf_hub_download
name = "ckpts/slat_flow_img2shape_dit_1_3B_1024_bf16"
cfgf = hf_hub_download("microsoft/TRELLIS.2-4B", name + ".json"); wf = hf_hub_download("microsoft/TRELLIS.2-4B", name + ".safetensors")
config = json.load(open(cfgf)); print("start", priv(), flush=True)
m = models.__getattr__(config["name"])(**config["args"]); print("built", priv(), flush=True)
sd = load_file(wf); print("load_file", priv(), flush=True)
m.load_state_dict(sd, strict=False); print("load_state_dict", priv(), flush=True)
del sd; gc.collect(); print("del sd", priv(), flush=True)
import ctypes
ctypes.cdll.msvcrt._heapmin(); print("heapmin", priv(), flush=True)
k = ctypes.windll.kernel32; h = k.GetCurrentProcess()
k.SetProcessWorkingSetSize(ctypes.c_void_p(h), ctypes.c_size_t(-1), ctypes.c_size_t(-1)); print("trim ws", priv(), flush=True)
print("params MiB", sum(p.numel()*p.element_size() for p in m.parameters())>>20)
print("buffers MiB", sum(b.numel()*b.element_size() for b in m.buffers())>>20)
seen=set(); tot=0
for t in list(m.parameters())+list(m.buffers()):
    s=t.untyped_storage()
    if s.data_ptr() not in seen: seen.add(s.data_ptr()); tot+=s.nbytes()
print("unique storage MiB", tot>>20)
with torch.no_grad():
    for p in m.parameters(): p.data = p.data.clone()
    for n,b in list(m.named_buffers()): pass
gc.collect(); ctypes.cdll.msvcrt._heapmin(); print("after clone", priv(), flush=True)
