"""Generate the Python and TypeScript data types from the Rust defaults.

Writes crates/ibkit-py/python/ibkit/_types.py and crates/ibkit-node/ts/types.ts.

The Rust example ``dump_defaults`` prints the serde form of every default data
type. This script turns each into a Python dataclass with the same field
names, field order and default values, so the Python and Rust types cannot
drift apart. Run from the repository root::

    cargo run -q -p ibkit --example dump_defaults > target/defaults.json
    uv run --no-project python scripts/gen_python_types.py target/defaults.json
"""

from __future__ import annotations

import json
import keyword
import sys
from pathlib import Path

OUT = Path('crates/ibkit-py/python/ibkit/_types.py')
TS_OUT = Path('crates/ibkit-node/ts/types.ts')

# Fields that the Python package turns into datetime objects (util.from_dict,
# IB._ticks, objects.realtime_bar); Rust holds seconds or a string.
DATETIMES: dict[str, str] = {
    'Execution': 'time',
    'Fill': 'time',
    'HistoricalTick': 'time',
    'HistoricalTickBidAsk': 'time',
    'HistoricalTickLast': 'time',
    'MktDepthData': 'time',
    'RealTimeBar': 'time',
    'TickData': 'time',
    'TradeLogEntry': 'time',
}

# Field name -> class of a nested object.
NESTED: dict[str, str] = {
    'softDollarTier': 'SoftDollarTier',
    'contract': 'Contract',
    'contractDetails': 'ContractDetails',
    'execution': 'Execution',
    'commissionReport': 'CommissionReport',
    'deltaNeutralContract': 'DeltaNeutralContract',
    'tickAttribBidAsk': 'TickAttribBidAsk',
    'tickAttribLast': 'TickAttribLast',
}

# Field name -> element class of a list.
LISTS: dict[str, str] = {
    'comboLegs': 'ComboLeg',
    'orderComboLegs': 'OrderComboLeg',
    'algoParams': 'TagValue',
    'smartComboRoutingParams': 'TagValue',
    'orderMiscOptions': 'TagValue',
    'secIdList': 'TagValue',
    'chartOptions': 'TagValue',
    'realTimeBarsOptions': 'TagValue',
    'conditions': 'OrderCondition',
    'orderAllocations': 'OrderAllocation',
    'ineligibilityReasonList': 'IneligibilityReason',
    'sessions': 'HistoricalSession',
}


def py_default(name: str, value: object) -> tuple[str, str]:
    """Return (annotation, default expression) for one field."""
    if name in NESTED:
        cls = NESTED[name]
        if value is None:
            return f'{cls} | None', 'None'
        return cls, f'field(default_factory=lambda: {cls}())'
    if isinstance(value, bool):
        return 'bool', repr(value)
    if isinstance(value, int):
        return 'int', repr(value)
    if isinstance(value, float):
        if value == 1.7976931348623157e308:
            return 'float', 'UNSET_DOUBLE'
        return 'float', repr(value)
    if isinstance(value, str):
        return 'str', repr(value)
    if isinstance(value, list):
        elem = LISTS.get(name, 'Any')
        return f'list[{elem}]', 'field(default_factory=list)'
    if value is None:
        return 'Any', 'None'
    if isinstance(value, dict):
        return 'dict', 'field(default_factory=dict)'
    raise ValueError(f'field {name} has unsupported default {value!r}')


def ts_type(name: str, value: object) -> str:
    """TypeScript type of one field."""
    if name in NESTED:
        return f'{NESTED[name]} | null' if value is None else NESTED[name]
    if isinstance(value, bool):
        return 'boolean'
    if isinstance(value, (int, float)):
        return 'number'
    if isinstance(value, str):
        return 'string'
    if isinstance(value, list):
        return f'{LISTS.get(name, "unknown")}[]'
    if isinstance(value, dict):
        return 'Record<string, unknown>'
    return 'unknown'


def write_ts(defaults: dict[str, dict]) -> None:
    lines = [
        '// Data types generated from the Rust types by scripts/gen_python_types.py.',
        '// Do not edit by hand. Inputs accept any subset of the fields (the Rust',
        '// side fills the rest with the defaults in DEFAULTS).',
        '',
        'export const UNSET_DOUBLE = 1.7976931348623157e308;',
        'export const UNSET_INTEGER = 2147483647;',
        '',
        'export interface TagValue {',
        '  tag: string;',
        '  value: string;',
        '}',
        '',
        '/** An order condition; `condType` selects the kind (see `order.ts`). */',
        'export interface OrderCondition {',
        '  condType: string;',
        '  conjunction: string;',
        '  [field: string]: unknown;',
        '}',
    ]
    for cls, fields in defaults.items():
        lines += ['', f'export interface {cls} {{']
        lines += [
            f'  {name}: {ts_type(name, value)};' for name, value in fields.items()
        ]
        lines.append('}')
    lines += [
        '',
        '/** The Rust default of every generated type. */',
        'export const DEFAULTS = {',
    ]
    for cls, fields in defaults.items():
        body = json.dumps(fields, separators=(', ', ': ')).replace(
            '1.7976931348623157e+308', 'UNSET_DOUBLE'
        )
        lines.append(f'  {cls}: {body} as {cls},')
    lines += ['} as const;', '']
    TS_OUT.parent.mkdir(parents=True, exist_ok=True)
    TS_OUT.write_text('\n'.join(lines))
    print(f'wrote {TS_OUT} with {len(defaults)} interfaces')


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(
            'usage: gen_python_types.py <defaults.json>; create the file with the dump_defaults example'
        )
    defaults: dict[str, dict] = json.loads(Path(sys.argv[1]).read_text())
    lines = [
        '"""Data classes generated from the Rust types. Do not edit by hand.',
        '',
        'Regenerate with scripts/gen_python_types.py.',
        '"""',
        '',
        'from __future__ import annotations',
        '',
        'import datetime as dt',
        'from dataclasses import dataclass, field',
        'from typing import Any, NamedTuple',
        '',
        'UNSET_DOUBLE: float = 1.7976931348623157e308',
        'UNSET_INTEGER: int = 2147483647',
        '#: The default of a datetime field.',
        'EPOCH: dt.datetime = dt.datetime(1970, 1, 1, tzinfo=dt.UTC)',
        '',
        '',
        'class TagValue(NamedTuple):',
        '    tag: str',
        '    value: str',
        '',
        '',
        '@dataclass',
        'class OrderCondition:',
        '    """Base of the order condition classes; see ``order.py``."""',
        '',
        '    condType: int = 0',
        '    conjunction: str = "a"',
        '',
    ]
    nested_map: dict[str, dict[str, str]] = {}
    lists_map: dict[str, dict[str, str]] = {}
    for cls, fields in defaults.items():
        lines += ['', '@dataclass', f'class {cls}:']
        nested_map[cls] = {k: NESTED[k] for k in fields if k in NESTED}
        lists_map[cls] = {k: LISTS[k] for k in fields if k in LISTS}
        for name, value in fields.items():
            if keyword.iskeyword(name):
                raise ValueError(f'{cls}.{name} is a Python keyword')
            ann, default = (
                ('dt.datetime', 'EPOCH')
                if DATETIMES.get(cls) == name
                else py_default(name, value)
            )
            lines.append(f'    {name}: {ann} = {default}')
    lines.append('')
    lines.append('# Class name -> {field: class of nested object}.')
    lines.append(f'NESTED: dict[str, dict[str, str]] = {nested_map!r}')
    lines.append('# Class name -> {field: class of list elements}.')
    lines.append(f'LISTS: dict[str, dict[str, str]] = {lists_map!r}')
    lines.append(f'GENERATED: tuple[str, ...] = {tuple(defaults)!r}')
    exported = ['UNSET_DOUBLE', 'UNSET_INTEGER', 'EPOCH', 'TagValue', *defaults]
    lines.append(f'__all__ = {sorted(exported, key=lambda n: (not n.isupper(), n))!r}')
    lines.append('')
    OUT.write_text('\n'.join(lines))
    print(f'wrote {OUT} with {len(defaults)} classes')
    write_ts(defaults)


if __name__ == '__main__':
    main()
