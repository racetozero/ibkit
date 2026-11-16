"""Generate the control API types of the TypeScript and Python SDKs from
crates/ibkit-gateway/openapi.json, so the SDKs cannot drift from the controller.
Run from the repository root (after `just openapi`)::

    uv run --no-project python scripts/gen_gateway_types.py

It writes crates/ibkit-node/ts/gateway_types.ts and
crates/ibkit-py/python/ibkit/_gateway_types.py. String enums become runtime
enums; tagged unions become discriminated unions. The Python file also
holds decoders that turn the enum strings of a response into enum members.
A schema construct that this script does not know stops it with an error.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

SOURCE = Path('crates/ibkit-gateway/openapi.json')
TS_OUT = Path('crates/ibkit-node/ts/gateway_types.ts')
PY_OUT = Path('crates/ibkit-py/python/ibkit/_gateway_types.py')
# TypeScript names that would be too generic next to the TWS types.
TS_NAMES = {
    'State': 'GatewayState',
    'Event': 'GatewayEvent',
    'Config': 'GatewayConfig',
    'Alert': 'GatewayAlert',
}
HEADER = 'Generated from crates/ibkit-gateway/openapi.json by scripts/gen_gateway_types.py. Do not edit by hand.'

Schema = dict[str, Any]


def fail(where: str, schema: object) -> None:
    sys.exit(
        f'{where}: the schema {json.dumps(schema)[:200]} has a form that '
        'gen_gateway_types.py does not know. Add it to the generator.'
    )


def ref_name(schema: Schema) -> str:
    return schema['$ref'].rsplit('/', 1)[1]


def string_values(schema: Schema) -> list[str] | None:
    """The values of a string enum (`enum`, or `oneOf` of enums and consts)."""
    if schema.get('type') == 'string' and 'enum' in schema:
        return list(schema['enum'])
    if 'oneOf' in schema and all(
        v.get('type') == 'string' and ('enum' in v or 'const' in v)
        for v in schema['oneOf']
    ):
        return [x for v in schema['oneOf'] for x in v.get('enum', [v.get('const')])]
    return None


def tagged(schema: Schema) -> tuple[str, list[Schema]] | None:
    """The tag field and variants of a tagged union of objects."""
    variants = schema.get('oneOf')
    if not variants or not all(v.get('type') == 'object' for v in variants):
        return None
    tags = {k for k, p in variants[0]['properties'].items() if 'const' in p}
    for variant in variants:
        tags &= {k for k, p in variant['properties'].items() if 'const' in p}
    if len(tags) != 1:
        return None
    return tags.pop(), variants


def pascal(value: str) -> str:
    return ''.join(
        part[:1].upper() + part[1:] for part in re.split(r'[_\W]+', value) if part
    )


def upper_snake(value: str) -> str:
    return re.sub(r'(?<=[a-z0-9])(?=[A-Z])', '_', value).upper()


# ---- TypeScript -------------------------------------------------------------


def q(value: object) -> str:
    """A TypeScript literal in the repository's quote style."""
    if isinstance(value, str):
        return "'" + value.replace('\\', '\\\\').replace("'", "\\'") + "'"
    return json.dumps(value)


def ts_type(schema: Schema, where: str) -> str:
    if '$ref' in schema:
        name = ref_name(schema)
        return TS_NAMES.get(name, name)
    if 'anyOf' in schema:
        return ' | '.join(ts_type(s, where) for s in schema['anyOf'])
    if 'const' in schema:
        return q(schema['const'])
    kind = schema.get('type')
    if isinstance(kind, list):
        return ' | '.join(ts_type({**schema, 'type': k}, where) for k in kind)
    if kind == 'string':
        return 'string'
    if kind == 'integer' or kind == 'number':
        return 'number'
    if kind == 'boolean':
        return 'boolean'
    if kind == 'null':
        return 'null'
    if kind == 'array':
        return f'{ts_type(schema["items"], where)}[]'
    fail(where, schema)
    raise AssertionError


def ts_comment(text: str | None, indent: str = '') -> list[str]:
    if not text:
        return []
    lines = text.replace('*/', '*\\/').splitlines()
    if len(lines) == 1:
        return [f'{indent}/** {lines[0]} */']
    return [
        f'{indent}/**',
        *(f'{indent} * {line}'.rstrip() for line in lines),
        f'{indent} */',
    ]


def ts_fields(schema: Schema, where: str, indent: str = '  ') -> list[str]:
    required = set(schema.get('required', []))
    out: list[str] = []
    for key, prop in schema['properties'].items():
        out += ts_comment(prop.get('description'), indent)
        optional = '' if key in required else '?'
        out.append(f'{indent}{key}{optional}: {ts_type(prop, f"{where}.{key}")};')
    return out


def inline(schema: Schema, where: str) -> str:
    """The fields of an object type on one line, without comments."""
    return '; '.join(
        f.rstrip(';')
        for f in ts_fields(schema, where, '')
        if not f.startswith(('/', ' *'))
    )


def ts_schema(name: str, schema: Schema) -> list[str]:
    out = ts_comment(schema.get('description'))
    values = string_values(schema)
    if values is not None:
        out.append(f'export enum {TS_NAMES.get(name, name)} {{')
        out += [f'  {pascal(v)} = {q(v)},' for v in values]
        return [*out, '}']
    union = tagged(schema)
    if union is not None:
        _, variants = union
        members = []
        for variant in variants:
            members.append(f'  | {{ {inline(variant, name)} }}')
        if schema.get('type') == 'object':
            base = inline(schema, name)
            return [
                *out,
                f'export type {TS_NAMES.get(name, name)} = {{ {base} }} & (',
                *members,
                ');',
            ]
        return [
            *out,
            f'export type {TS_NAMES.get(name, name)} =',
            *members[:-1],
            f'{members[-1]};',
        ]
    if schema.get('type') == 'object' and 'properties' in schema:
        return [
            *out,
            f'export interface {TS_NAMES.get(name, name)} {{',
            *ts_fields(schema, name),
            '}',
        ]
    if schema.get('type') == 'object' and isinstance(
        schema.get('additionalProperties'), dict
    ):
        value = ts_type(schema['additionalProperties'], name)
        return [
            *out,
            f'export type {TS_NAMES.get(name, name)} = Record<string, {value}>;',
        ]
    fail(name, schema)
    raise AssertionError


# ---- Python -----------------------------------------------------------------


def py_type(schema: Schema, where: str) -> str:
    if '$ref' in schema:
        return ref_name(schema)
    if 'anyOf' in schema:
        return ' | '.join(py_type(s, where) for s in schema['anyOf'])
    if 'const' in schema:
        return f'Literal[{schema["const"]!r}]'
    kind = schema.get('type')
    if isinstance(kind, list):
        return ' | '.join(py_type({**schema, 'type': k}, where) for k in kind)
    simple = {
        'string': 'str',
        'integer': 'int',
        'number': 'float',
        'boolean': 'bool',
        'null': 'None',
    }
    if kind in simple:
        return simple[kind]
    if kind == 'array':
        return f'list[{py_type(schema["items"], where)}]'
    fail(where, schema)
    raise AssertionError


def py_doc(text: str | None, indent: str = '    ') -> list[str]:
    if not text:
        return []
    text = text.replace('\\', '\\\\').replace('"""', '\\"\\"\\"')
    lines = text.splitlines()
    if len(lines) == 1:
        return [f'{indent}"""{lines[0]}"""', '']
    return [
        f'{indent}"""{lines[0]}',
        *(f'{indent}{line}'.rstrip() for line in lines[1:]),
        f'{indent}"""',
        '',
    ]


class Python:
    def __init__(self, schemas: dict[str, Schema]) -> None:
        self.schemas = schemas
        self.enums = {n for n, s in schemas.items() if string_values(s) is not None}
        self.decoders: dict[str, bool] = {}

    def needs_decoder(self, schema: Schema) -> bool:
        """Whether values of `schema` hold an enum string."""
        if '$ref' in schema:
            name = ref_name(schema)
            if name in self.enums:
                return True
            if name not in self.decoders:
                self.decoders[name] = False
                self.decoders[name] = self.needs_decoder(self.schemas[name])
            return self.decoders[name]
        if 'anyOf' in schema:
            return any(self.needs_decoder(s) for s in schema['anyOf'])
        if schema.get('type') == 'array':
            return self.needs_decoder(schema['items'])
        if 'oneOf' in schema and string_values(schema) is None:
            return any(
                self.needs_decoder(v) for v in schema['oneOf']
            ) or self.needs_decoder({k: v for k, v in schema.items() if k != 'oneOf'})
        if 'properties' in schema:
            return any(self.needs_decoder(p) for p in schema['properties'].values())
        return False

    def decode_expr(self, schema: Schema, value: str) -> str | None:
        """An expression that decodes `value` of `schema`, or None to keep it."""
        if not self.needs_decoder(schema):
            return None
        if '$ref' in schema:
            name = ref_name(schema)
            return (
                f'_member({name}, {value})'
                if name in self.enums
                else f'_decode_{name}({value})'
            )
        if 'anyOf' in schema:
            parts = [s for s in schema['anyOf'] if s.get('type') != 'null']
            if len(parts) != 1:
                fail(value, schema)
            inner = self.decode_expr(parts[0], value)
            return f'None if {value} is None else {inner}'
        if schema.get('type') == 'array':
            inner = self.decode_expr(schema['items'], '_item')
            return f'[{inner} for _item in {value}]'
        fail(value, schema)
        raise AssertionError

    def field_decoders(self, schema: Schema, indent: str) -> list[str]:
        out = []
        required = set(schema.get('required', []))
        for key, prop in schema.get('properties', {}).items():
            expr = self.decode_expr(prop, f'value[{key!r}]')
            if expr is None:
                continue
            line = f'value[{key!r}] = {expr}'
            if key in required:
                out.append(f'{indent}{line}')
            else:
                out += [f'{indent}if {key!r} in value:', f'{indent}    {line}']
        return out

    def typed_dict(
        self, name: str, schema: Schema, base: str = 'TypedDict'
    ) -> list[str]:
        required = set(schema.get('required', []))
        fields = []
        for key, prop in schema.get('properties', {}).items():
            kind = py_type(prop, f'{name}.{key}')
            fields.append(
                f'    {key}: {kind if key in required else f"NotRequired[{kind}]"}'
            )
        body = py_doc(schema.get('description')) + fields
        while body and body[-1] == '':
            body.pop()
        return [f'class {name}({base}):', *(body or ['    pass'])]

    def schema(self, name: str, schema: Schema) -> list[str]:
        values = string_values(schema)
        if values is not None:
            out = [f'class {name}(str, Enum):', *py_doc(schema.get('description'))]
            out += [f'    {upper_snake(v)} = {v!r}' for v in values]
            return out
        union = tagged(schema)
        if union is not None:
            tag, variants = union
            out: list[str] = []
            base = 'TypedDict'
            if schema.get('type') == 'object':
                base = f'_{name}Base'
                common = {
                    k: v for k, v in schema.items() if k not in ('oneOf', 'description')
                }
                out += self.typed_dict(base, common) + ['', '']
            members = []
            for variant in variants:
                member = f'{name}{pascal(variant["properties"][tag]["const"])}'
                members.append(member)
                out += self.typed_dict(member, variant, base) + ['', '']
            out += py_doc(schema.get('description'), '')
            out += [
                f'{name} = (',
                *(
                    f'    {m}' if i == 0 else f'    | {m}'
                    for i, m in enumerate(members)
                ),
                ')',
            ]
            return out
        if schema.get('type') == 'object' and 'properties' in schema:
            return self.typed_dict(name, schema)
        if schema.get('type') == 'object' and isinstance(
            schema.get('additionalProperties'), dict
        ):
            value = py_type(schema['additionalProperties'], name)
            return [
                *py_doc(schema.get('description'), ''),
                f'{name} = dict[str, {value}]',
            ]
        fail(name, schema)
        raise AssertionError

    def decoder(self, name: str, schema: Schema) -> list[str]:
        out = [f'def _decode_{name}(value: Any) -> {name}:']
        union = tagged(schema)
        body = self.field_decoders(schema, '    ')
        if union is not None:
            tag, variants = union
            for variant in variants:
                lines = self.field_decoders(variant, '        ')
                if lines:
                    body += [
                        f'    if value[{tag!r}] == {variant["properties"][tag]["const"]!r}:',
                        *lines,
                    ]
        return [*out, *body, '    return value']


def main() -> None:
    document = json.loads(SOURCE.read_text())
    schemas: dict[str, Schema] = document['components']['schemas']

    ts = [f'// {HEADER}', '']
    for name, schema in schemas.items():
        ts += ts_schema(name, schema) + ['']
    TS_OUT.write_text('\n'.join(ts).rstrip() + '\n')

    py = Python(schemas)
    lines = [
        f'"""{HEADER}"""',
        '',
        'from __future__ import annotations',
        '',
        'from enum import Enum',
        'from typing import Any, Literal, TypedDict, TypeVar',
        '',
        '',
    ]
    for name, schema in schemas.items():
        lines += py.schema(name, schema) + ['', '']
    lines += [
        '# Decoders: turn the enum strings of a decoded JSON value into enum members.',
        '',
        '',
        "_E = TypeVar('_E', bound=Enum)",
        '',
        '',
        'def _member(enum: type[_E], value: str) -> _E:',
        '    """The member of `enum` with `value`; a value that a newer controller',
        '    added stays a plain string."""',
        '    try:',
        '        return enum(value)',
        '    except ValueError:',
        '        return value  # type: ignore[return-value]',
        '',
        '',
    ]
    for name, schema in schemas.items():
        if name not in py.enums and py.needs_decoder({'$ref': f'#/{name}'}):
            lines += py.decoder(name, schema) + ['', '']
    text = '\n'.join(lines).rstrip() + '\n'
    if 'NotRequired[' in text:
        text = text.replace('Literal, TypedDict', 'Literal, NotRequired, TypedDict')
    PY_OUT.write_text(text)
    print(f'wrote {TS_OUT} and {PY_OUT}')


if __name__ == '__main__':
    main()
