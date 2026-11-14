#!/usr/bin/env python3
"""A JSON Schema validator narrow enough to be read in one sitting.

Stdlib only, on purpose. This repo's conventions keep `check-python` and
`check-docs` runnable on a host with nothing installed, and taking a
`jsonschema` dependency to make a gate would spend that (WI 1926, CD-24). The
alternative was writing a small engine for the constructs this repo's schemas
actually use — which is what this is.

**It is not a general validator and must never pretend to be one.** It
implements exactly the keywords `contracts/schemas/*.json` use today, and
raises `SchemaError` on any keyword it does not implement. That refusal is the
whole safety property: a schema that grows `anyOf` fails the gate with "extend
me", rather than being silently under-checked. A validator that ignores what
it does not understand reports success it has not earned, which is the failure
this repo already met once in prose (WI 2781) and is not repeating in code.

Two spellings of the same idea, both in the schema file:

* `examples` — standard JSON Schema annotation. Every entry MUST validate.
* `x-counterexamples` — this repo's own, ignored by every real validator:
  a list of `{"record": …, "why": "…"}`. Every entry MUST FAIL, and the gate
  says which one wrongly passed. These are the negative tests, carried next to
  the thing they test rather than in a test file that drifts away from it.

Records are the DECODED record in both cases. HASH-shaped feeds (`claude:*`,
`ghcp:*`) arrive off the wire as strings for every field; the schemas describe
them decoded and so do these examples. See contracts/rules.md.
"""

from __future__ import annotations

import re
from typing import Any

#: Keywords that carry no assertion. Present so an unknown keyword is an
#: error rather than an oversight — see `ANNOTATIONS | ASSERTIONS` below.
ANNOTATIONS = {
    "$schema",
    "$id",
    "$comment",
    "title",
    "description",
    "examples",
    "x-counterexamples",
}

#: Keywords this validator actually implements. Adding one here without
#: implementing it below turns the gate off for that keyword, which is the
#: single most dangerous edit anyone can make to this file.
ASSERTIONS = {
    "type",
    "enum",
    "const",
    "properties",
    "required",
    "additionalProperties",
    "propertyNames",
    "maxProperties",
    "items",
    "maxItems",
    "pattern",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "exclusiveMinimum",
}

KNOWN = ANNOTATIONS | ASSERTIONS

TYPE_NAMES = {"object", "array", "string", "number", "integer", "boolean", "null"}


class SchemaError(Exception):
    """The *schema* is wrong or out of this validator's range.

    Distinct from a validation failure on purpose: a record that violates its
    schema is news about the record, and a schema this file cannot read is
    news about this file.
    """


def _is_type(value: Any, name: str) -> bool:
    if name == "object":
        return isinstance(value, dict)
    if name == "array":
        return isinstance(value, list)
    if name == "string":
        return isinstance(value, str)
    if name == "boolean":
        return isinstance(value, bool)
    if name == "null":
        return value is None
    # `True` is an `int` in Python and is not a number in JSON Schema. Every
    # numeric check below has to say so, or `{"type": "number"}` silently
    # accepts a boolean.
    if name == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if name == "integer":
        if isinstance(value, bool):
            return False
        if isinstance(value, int):
            return True
        # JSON has one number type: 1.0 IS an integer, and a publisher that
        # emits `"pid": 4132.0` is not violating anything.
        return isinstance(value, float) and value.is_integer()
    raise SchemaError(f"unknown type name {name!r}")


def _same(a: Any, b: Any) -> bool:
    """JSON equality, which is not Python's.

    `True == 1` in Python, so a `{"const": true}` field would accept `1` and a
    `{"enum": [0, 1]}` field would accept `false`. Both are real shapes in this
    repo (`stale`, `scoped_active`), so the distinction is not academic.
    """
    if isinstance(a, bool) != isinstance(b, bool):
        return False
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(_same(a[k], b[k]) for k in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(_same(x, y) for x, y in zip(a, b))
    if isinstance(a, bool) or isinstance(b, bool):
        return a is b
    return a == b


def _pattern_ok(pattern: str, value: str) -> bool:
    """`pattern`, with ECMA-262's end-of-input rather than Python's.

    JSON Schema patterns are ECMA-262 with no `m` flag, where `$` means end of
    input. Python's `$` ALSO matches just before a trailing newline — so
    `"kai\\n"` satisfies `^[A-Za-z0-9._-]{1,63}$` under `re` and violates it
    under the contract. A host token with a stray newline is precisely the
    near-miss this gate exists to catch, so the gap is closed by requiring an
    anchored pattern's match to reach the end of the value.
    """
    try:
        match = re.search(pattern, value)
    except re.error as exc:  # a pattern Python cannot compile is a schema bug
        raise SchemaError(f"pattern {pattern!r} does not compile: {exc}") from exc
    if match is None:
        return False
    anchored_end = pattern.endswith("$") and not pattern.endswith("\\$")
    return not anchored_end or match.end() == len(value)


def _check_keywords(schema: dict, where: str) -> None:
    unknown = sorted(set(schema) - KNOWN)
    if unknown:
        raise SchemaError(
            f"{where}: keyword(s) {', '.join(unknown)} are not implemented by "
            "scripts/jsonschema_mini.py. Implement them there (and add a "
            "counterexample that proves the new check bites) rather than "
            "letting this schema go unchecked"
        )


#: Where a subschema can sit, by how the parent holds it.
_SUBSCHEMA_VALUES = ("items", "additionalProperties", "propertyNames")
_SUBSCHEMA_MAPS = ("properties",)


def check_keywords_deep(schema: Any, where: str, path: str = "") -> None:
    """Walk every subschema position and refuse anything unimplemented.

    Separate from [`validate`] and not optional: `validate` only descends into
    a property an example actually carries, so a schema could grow `anyOf` on
    a rarely-filled field and go unchecked for as long as no example filled
    it. This walk does not depend on the records at all.
    """
    if isinstance(schema, bool):
        return
    if not isinstance(schema, dict):
        raise SchemaError(f"{where}:{path or '<root>'} is not a schema")
    _check_keywords(schema, f"{where}:{path or '<root>'}")
    names = schema.get("type", [])
    for name in [names] if isinstance(names, str) else names:
        if name not in TYPE_NAMES:
            raise SchemaError(f"{where}:{path or '<root>'}: unknown type name {name!r}")
    for key in _SUBSCHEMA_VALUES:
        if key in schema:
            check_keywords_deep(schema[key], where, f"{path}.{key}")
    for key in _SUBSCHEMA_MAPS:
        for name, child in schema.get(key, {}).items():
            check_keywords_deep(child, where, f"{path}.{key}.{name}")


def validate(record: Any, schema: Any, path: str = "", where: str = "<schema>") -> list[str]:
    """Every way `record` violates `schema`, as human sentences.

    `path` is the dotted position inside the record; `where` names the schema
    file, for `SchemaError` messages.
    """
    if isinstance(schema, bool):
        # `"additionalProperties": false` reaches here as a subschema.
        return [] if schema else [f"{path or '<record>'}: not allowed here"]
    if not isinstance(schema, dict):
        raise SchemaError(f"{where}: {path or '<root>'} is not a schema")

    _check_keywords(schema, f"{where}:{path or '<root>'}")
    at = path or "<record>"
    errors: list[str] = []

    if "type" in schema:
        names = schema["type"]
        names = [names] if isinstance(names, str) else names
        for name in names:
            if name not in TYPE_NAMES:
                raise SchemaError(f"{where}:{at}: unknown type name {name!r}")
        if not any(_is_type(record, name) for name in names):
            got = "null" if record is None else type(record).__name__
            return errors + [f"{at}: expected {'/'.join(names)}, got {got}"]

    if "enum" in schema and not any(_same(record, v) for v in schema["enum"]):
        errors.append(f"{at}: {record!r} is not one of {schema['enum']}")
    if "const" in schema and not _same(record, schema["const"]):
        errors.append(f"{at}: must be {schema['const']!r}, got {record!r}")

    if isinstance(record, str):
        if "pattern" in schema and not _pattern_ok(schema["pattern"], record):
            errors.append(f"{at}: {record!r} does not match {schema['pattern']}")
        if "minLength" in schema and len(record) < schema["minLength"]:
            errors.append(f"{at}: shorter than minLength {schema['minLength']}")
        if "maxLength" in schema and len(record) > schema["maxLength"]:
            errors.append(f"{at}: longer than maxLength {schema['maxLength']}")

    if isinstance(record, (int, float)) and not isinstance(record, bool):
        if "minimum" in schema and record < schema["minimum"]:
            errors.append(f"{at}: {record} is below minimum {schema['minimum']}")
        if "maximum" in schema and record > schema["maximum"]:
            errors.append(f"{at}: {record} is above maximum {schema['maximum']}")
        if "exclusiveMinimum" in schema and record <= schema["exclusiveMinimum"]:
            errors.append(
                f"{at}: {record} is not above exclusiveMinimum "
                f"{schema['exclusiveMinimum']}"
            )

    if isinstance(record, list):
        if "maxItems" in schema and len(record) > schema["maxItems"]:
            errors.append(f"{at}: {len(record)} items exceeds maxItems {schema['maxItems']}")
        if "items" in schema:
            for i, item in enumerate(record):
                errors += validate(item, schema["items"], f"{at}[{i}]", where)

    if isinstance(record, dict):
        for name in schema.get("required", []):
            if name not in record:
                errors.append(f"{at}: missing required field {name!r}")
        if "maxProperties" in schema and len(record) > schema["maxProperties"]:
            errors.append(
                f"{at}: {len(record)} fields exceeds maxProperties "
                f"{schema['maxProperties']}"
            )
        properties = schema.get("properties", {})
        for name, value in record.items():
            child = f"{at}.{name}" if at != "<record>" else name
            if "propertyNames" in schema:
                errors += [
                    f"{at}: field name {name!r} is not allowed — {e.split(': ', 1)[-1]}"
                    for e in validate(name, schema["propertyNames"], child, where)
                ]
            if name in properties:
                errors += validate(value, properties[name], child, where)
            elif "additionalProperties" in schema:
                extra = schema["additionalProperties"]
                if extra is False:
                    errors.append(f"{at}: unexpected field {name!r}")
                elif extra is not True:
                    errors += validate(value, extra, child, where)

    return errors


def check_schema_file(name: str, schema: dict) -> list[str]:
    """Hold one schema's own `examples` and `x-counterexamples` to it.

    Returns gate errors, already phrased for `scripts/check.py`. A schema with
    no examples is an error too: an unexercised schema is the state this whole
    gate exists to leave behind, and "added a feed, forgot the examples" is the
    way back into it.
    """
    errors: list[str] = []
    where = f"contracts/schemas/{name}"

    # Before anything else, and regardless of what the examples exercise: a
    # keyword this file does not implement must stop the gate, not be skipped.
    try:
        check_keywords_deep(schema, where)
    except SchemaError as exc:
        return [str(exc)]

    examples = schema.get("examples")
    counters = schema.get("x-counterexamples")

    if not isinstance(examples, list) or not examples:
        return [
            f"{where}: no `examples` array — every schema carries at least one "
            "valid record, or nothing here proves it describes anything"
        ]
    if not isinstance(counters, list) or not counters:
        return [
            f"{where}: no `x-counterexamples` array — a schema that has never "
            "been seen to REJECT anything is not a gate (see this repo's "
            "'negative-test it' rule)"
        ]

    try:
        for i, example in enumerate(examples):
            for problem in validate(example, schema, "", where):
                errors.append(f"{where}: examples[{i}] should be valid but {problem}")

        for i, counter in enumerate(counters):
            if not isinstance(counter, dict) or "record" not in counter:
                errors.append(
                    f"{where}: x-counterexamples[{i}] must be "
                    '{"record": …, "why": "…"}'
                )
                continue
            if not str(counter.get("why", "")).strip():
                errors.append(
                    f"{where}: x-counterexamples[{i}] has no `why` — a negative "
                    "case whose point nobody wrote down gets deleted by the next "
                    "person who cannot see it"
                )
            if not validate(counter["record"], schema, "", where):
                errors.append(
                    f"{where}: x-counterexamples[{i}] was ACCEPTED but must be "
                    f"rejected — {counter.get('why', '(no why given)')}"
                )
    except SchemaError as exc:
        errors.append(str(exc))

    return errors
