#!/usr/bin/env python3
"""Every user-visible English string that is not behind t().

Four patterns, because each earlier round of this missed a different one:
JSX text nodes (any length, including single words), the props that render,
prose literals anywhere in the file, and text derived from an enum.
"""
import re, pathlib, sys

node = re.compile(r">\s*([A-Z][A-Za-z][^<>{}]*?)\s*<")
prop = re.compile(r"\b(?:title|label|placeholder|description|emptyLabel|confirmLabel|secondaryHeader|ariaLabel|aria-label)=\"([^\"]{3,})\"")
inline = re.compile(r"^\s{4,}([A-Z][A-Za-z][^<>{}]*?)\s*$")
prose = re.compile(r"""['"`]([A-Za-z][A-Za-z0-9,.'’()\-]*(?: [A-Za-z0-9,.'’()\-]+){1,}[.?!]?)['"`]""")
derived = re.compile(r"\{[^}]*\b(?:toLowerCase|toUpperCase)\(\)[^}]*\}")

ALLOW = ('LocalTrack','Chrome','SQLite','http','chrome-extension','apps/','pnpm ',
         'localtrack-native-host','fa-IR','en-u-ca','2-digit','Vazirmatn','rotate(',
         'XLSX','CSV','URL','Promise','KB','MB','GB','journalMode')
technical = re.compile(r"^(?:[a-z-]+ ?)+$")

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else '.')
findings = []
for path in sorted(root.rglob('*.tsx')) + sorted(root.rglob('*.ts')):
    rel = str(path.relative_to(root))
    if '__tests__' in rel or rel.startswith('i18n/'):
        continue
    for n, line in enumerate(path.read_text().splitlines(), 1):
        st = line.strip()
        # Comments are not interface text — including JSX ones.
        if st.startswith(('//', '*', '/*', '{/*')) or '{/*' in line:
            continue
        # A line that already calls t() has had its strings extracted; the
        # remaining quotes on it are keys and toast kinds, not prose.
        if "t('" in line or 't(`' in line:
            continue
        # SVG geometry is not language.
        if re.search(r'\b[Md]\s?-?\d', line) and ('path' in line or 'd=' in line):
            continue
        for rx in (node, prop, inline, prose):
            if rx is prose and ('className' in line or 'aria-hidden' in line):
                continue
            for m in rx.finditer(line):
                found = m.group(1).strip()
                if any(a in found for a in ALLOW) or technical.match(found):
                    continue
                if found.startswith('t(') or '{' in found or len(found) < 3:
                    continue
                findings.append((rel, n, found[:70]))
        # A value the machine names (WAL, a process name) is an identifier,
        # not a word to translate.
        if ('className' not in line and derived.search(line) and 'i18n' not in rel
                and not any(a in line for a in ALLOW)):
            findings.append((rel, n, st[:70]))

for rel, n, text in findings:
    print(f"{rel}:{n}  {text}")
print(f"\n{len(findings)} user-visible strings not behind t()")
sys.exit(1 if findings else 0)
