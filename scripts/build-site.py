#!/usr/bin/env python3
"""Build an allowlisted static artifact without deleting any existing directory."""
from pathlib import Path
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FILES = ('index.html', 'indexv2.html', 'manifest.json', 'sw.js')
DIRECTORIES = ('res', 'order', 'offseter', 'pixmix', 'transfer')
EXTENSIONS = {'.html', '.css', '.js', '.mjs', '.json', '.svg', '.png', '.jpg', '.jpeg', '.gif', '.webp', '.ico', '.woff', '.woff2', '.ttf'}


def build():
    # Validate all sources before creating the temporary output.
    selected = [ROOT / name for name in FILES]
    for name in DIRECTORIES:
        folder = ROOT / name
        if not folder.is_dir() or folder.is_symlink():
            raise ValueError(f'Invalid static directory: {name}')
        for p in folder.rglob('*'):
            if p.is_symlink():
                raise ValueError(f'Symlinks are not allowed: {p.relative_to(ROOT)}')
            if p.is_file() and p.suffix.lower() in EXTENSIONS:
                if not any(part.startswith('.') for part in p.relative_to(ROOT).parts):
                    selected.append(p)
    for p in selected:
        if p.is_symlink() or not p.is_file() or not p.resolve().is_relative_to(ROOT):
            raise ValueError(f'Invalid static file: {p}')
    output = Path(tempfile.mkdtemp(prefix='compressor-eo-'))
    for p in selected:
        target = output / p.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(p, target)
    return output


if __name__ == '__main__':
    print(build())
