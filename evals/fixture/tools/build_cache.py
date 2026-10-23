"""Compile artifacts once and reuse them for every later step."""


def build_cache(root):
    cache = {}
    for step in pipeline_steps(root):
        cache[step.name] = compile_artifacts(step)
    return cache


def pipeline_steps(root):
    return sorted(root.rglob("step.toml"))


def compile_artifacts(step):
    return {"name": step.stem, "inputs": step.read_text()}
