"""Generate crates/ibkit-py/python/ibkit/_ibkit.pyi from the built native
module, so type checkers see every class, method and parameter of it. The
public types live in the Python wrapper modules; the native methods return
``Any`` (Python objects built from the Rust values). Build the module first,
then run from the repository root::

    cd crates/ibkit-py && uv run maturin develop && cd ../..
    uv run --project crates/ibkit-py python scripts/gen_python_stub.py
"""

from __future__ import annotations

import inspect
from pathlib import Path

from ibkit import _ibkit

OUT = Path('crates/ibkit-py/python/ibkit/_ibkit.pyi')


def parameters(signature: inspect.Signature, first: str | None) -> str:
    out: list[str] = [] if first is None else [first]
    star = False
    for p in signature.parameters.values():
        if p.name in ('self', 'cls', '$self', '$cls'):
            continue
        if p.kind is p.VAR_POSITIONAL:
            out.append(f'*{p.name}: Any')
            star = True
            continue
        if p.kind is p.VAR_KEYWORD:
            out.append(f'**{p.name}: Any')
            continue
        if p.kind is p.KEYWORD_ONLY and not star:
            out.append('*')
            star = True
        default = '' if p.default is p.empty else ' = ...'
        out.append(f'{p.name}: Any{default}')
    return ', '.join(out)


def signature_of(obj: object) -> inspect.Signature:
    try:
        return inspect.signature(obj)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return inspect.Signature(
            [
                inspect.Parameter('args', inspect.Parameter.VAR_POSITIONAL),
                inspect.Parameter('kwargs', inspect.Parameter.VAR_KEYWORD),
            ]
        )


def main() -> None:
    lines = [
        '# Generated from the built ibkit._ibkit by scripts/gen_python_stub.py. Do not edit by hand.',
        '',
        'from typing import Any',
    ]
    for name in sorted(vars(_ibkit)):
        obj = vars(_ibkit)[name]
        if name.startswith('_'):
            continue
        if isinstance(obj, type):
            lines += ['', f'class {name}:']
            body: list[str] = []
            if obj.__text_signature__ is not None:
                sig = signature_of(obj)
                body.append(f'    def __init__({parameters(sig, "self")}) -> None: ...')
            for attr in sorted(vars(obj)):
                if attr.startswith('__'):
                    continue
                raw = vars(obj)[attr]
                kind = type(raw).__name__
                if kind == 'getset_descriptor':
                    body.append(f'    {attr}: Any')
                elif isinstance(raw, staticmethod):
                    sig = signature_of(getattr(obj, attr))
                    body += [
                        '    @staticmethod',
                        f'    def {attr}({parameters(sig, None)}) -> Any: ...',
                    ]
                elif kind == 'classmethod_descriptor':
                    sig = signature_of(getattr(obj, attr))
                    body += [
                        '    @classmethod',
                        f'    def {attr}({parameters(sig, "cls")}) -> Any: ...',
                    ]
                elif callable(raw):
                    sig = signature_of(raw)
                    body.append(
                        f'    def {attr}({parameters(sig, "self")}) -> Any: ...'
                    )
                else:
                    body.append(f'    {attr}: Any')
            lines += body or ['    ...']
        elif callable(obj):
            sig = signature_of(obj)
            lines += ['', f'def {name}({parameters(sig, None)}) -> Any: ...']
    OUT.write_text('\n'.join(lines) + '\n')
    print(f'wrote {OUT}')


if __name__ == '__main__':
    main()
