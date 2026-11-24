# Project Configuration

A project config tells svlang-lint which code is yours and how each rule should
behave. It is written in Python as ordinary classes, so a team can keep a
shared base config and each block can override just what it needs.

```bash
# Auto-discover sources/filelists/UVM and write .svlang-lint/config.py + baseline.json
svlang lint --init

# Or write a commented starter config template, then edit it
svlang lint --dump-config .svlang-lint/config.py
```

svlang-lint looks for configuration files in the working directory and its parents (first hit wins):
1. `.svlang-lint/config.py`
2. `.svlang-lint/config.json`
3. `svlang_lint_config.py`
4. `.svlang-lint.py`
5. `.svlang-lint.json`
6. `svlang-lint.json`

You can also specify `--config <file>` explicitly. A config that fails to load stops the run with exit
code 2 instead of silently linting with defaults.

In addition, `.svlang-lint/waivers.json` and `.svlang-lint/baseline.json` are automatically discovered if present.

## Example

```python
from svlang_lint.config import ProjectConfig, RuleConfig, Waiver, Severity


class Project(ProjectConfig):
    # Third-party code: compiled so your testbench elaborates, never reported.
    library_paths = ["$UVM_HOME", "vip/**"]

    # Your own files that should not be reported (generated code, etc.).
    exclude_paths = ["**/generated/**"]

    # One class per rule. The class name is the rule name, or set `rule`.
    class MaxLineLength(RuleConfig):
        max_columns = 140                    # rule option

    class NoCasex(RuleConfig):
        rule = "RTL-DAT-002"
        severity = Severity.Error
        exclude_paths = ["legacy/**"]       # this rule only

    class GenericReportId(RuleConfig):
        enabled = False

    waivers = [
        Waiver("UVM-OBJ-002", path="tb/stress_seq.sv", line=142,
               reason="Objection in loop verified for stress bench",
               message="objection"),
        LegacyMacros,
    ]


# A reusable waiver is a class; subclass it to widen or narrow it.
class LegacyMacros(Waiver):
    rules = ["SVL-STY-001", "OneStatementPerLine"]
    paths = ["**/legacy/**"]
    reason = "Vendor macros we cannot edit"
    owner = "dv-infra"
    expires = "2027-06-30"     # stops applying after this date, with a warning
```

(Define `LegacyMacros` above `Project` in a real file; it is shown after for reading order.)

## Sharing and overriding

Configs can import a base class from a file next to them (the config's folder
is on the import path):

```python
# team_base.py
class CompanyBase(ProjectConfig):
    library_paths = ["$UVM_HOME", "$VIP_ROOT"]

    class MaxLineLength(RuleConfig):
        max_columns = 120

# svlang_lint_config.py
from team_base import CompanyBase

class DmaBlock(CompanyBase):
    library_paths = CompanyBase.library_paths + ["third_party/axi_vip"]

    class MaxLineLength(CompanyBase.MaxLineLength):   # override one option
        max_columns = 160

    waivers = CompanyBase.waivers + [
        Waiver("SVL-STY-001", path="**/legacy/**", reason="Vendor macros"),
    ]
```

Rule classes are inherited; defining one with the same rule key replaces the
parent's. If a file defines several unrelated `ProjectConfig` classes, pick one
with `config = DmaBlock`. For anything dynamic, override `rule_configs()` or
`to_dict()`.

## Where the config is found, and sub-tree configs

svlang-lint looks for `.svlang-lint/config.py` (and the other names above) from the
working directory upwards; if there is none, from the folder of the first
source file upwards, so a run started in a separate `sim/` folder still finds
the project's config. `.svlang-lint/waivers.json` and `.svlang-lint/baseline.json`
are read from the same project root. `--no-config` skips the search.

A sub-tree such as a VIP or a block-level env can keep its own
`.svlang-lint/config.py` that builds on the project's. `parent_project()` returns
the project class of the next `.svlang-lint/config.py` further up, with its
relative paths anchored to the parent's root:

```python
# blocks/dma/.svlang-lint/config.py
from svlang_lint.config import parent_project, RuleConfig

class DmaBlock(parent_project()):
    exclude_paths = parent_project().exclude_paths + ["gen/**"]

    class MaxLineLength(RuleConfig):
        max_columns = 160
```

Running from `blocks/dma/` prints the layers that were loaded.

## Reference

| Setting | Meaning |
| :--- | :--- |
| `sources` | Files or globs to compile when the command line names none (`svlang-lint` alone then lints the project). The language server compiles these around the file you are editing. |
| `filelists` | `.f` file lists (paths inside are relative to the list), as with `-F`. |
| `include_dirs` | `+incdir` directories; apply to every run, also when files are given on the command line. |
| `defines` | `"NAME"` or `"NAME=VALUE"` macro definitions; apply to every run. |
| `library_paths` | Globs for third-party code (UVM, VIP). Files are still compiled; violations in them are dropped. |
| `exclude_paths` | Globs for your own files whose violations should be dropped. |
| `report_library_macro_expansions` | Default `False`: code produced by a library macro (`` `uvm_object_utils `` etc.) inside your file counts as library code. |
| `preset` | The rule set when `--preset` is not given: `"default"` (soundness, methodology, performance and security), `"all"`, `"style"` and the other preset names. `--preset` on the command line wins. |
| `waivers` | `Waiver(...)` objects or `Waiver` subclasses with `rules`, `paths`, `line`, `reason` (mandatory), `owner`, `expires` (YYYY-MM-DD), and optional `message`. Empty `rules` / `paths` mean every rule / file. `message` is a regular expression searched against the diagnostic text; the waiver applies only when rule, path, line, and that expression all match. An invalid expression is an error that names the config file and the waiver. With `severity` (`Severity.Error` / `Warning` / `Info`, or the name as a string; `"severity"` in a waivers JSON file) the waiver does not hide the finding but reports it at that severity, with `[re-graded by ...]` after its message, for example to keep known issues visible at Info while a fix is tracked. |
| `style`, `naming` | Same dicts as the legacy JSON `style` / `naming` blocks. |
| `compat` | Legacy compatibility dict accepted for backwards compatibility with older configs (ignored by the native Rust engine). |
| `RuleConfig.rule` | Rule ID (`"SVL-STY-013"`), prefix wildcard (`"UVM-NAM-*"`), or rule name (`"MaxLineLength"`); defaults to the class name. Exact rule entries take precedence over wildcard entries. |
| `RuleConfig.enabled` | `False` skips the rule entirely (also for Python plugin rules). A rule you configure in any other way runs even when the preset leaves it out: a `RuleConfig` for a style rule, or a `style` / `naming` block, turns those checks on without `preset = "all"`. `--preset` on the command line narrows the run to that preset. |
| `RuleConfig.severity` | `Severity.Error`, `Severity.Warning` or `Severity.Info`. |
| `RuleConfig.exclude_paths` | Globs where only this rule is silenced. |
| `RuleConfig.only_paths` | Globs where this rule reports; elsewhere it is silent. For example strict naming only under `env/**`, or a rule kept out of generated code without listing every other folder. `exclude_paths` still applies inside them. |
| any other attribute | A rule option, e.g. `max_columns` (SVL-STY-013), `max_complexity` / `warning_complexity` (SVL-MNT-055). |

**The UVM library needs no configuration.** When a compile holds `uvm_pkg.sv` and files outside
its folder, that folder is library code: nothing is reported in its files, nor for what its macros expand to in your
code (the routines of `` `uvm_object_utils ``, the method of `` `uvm_field_int ``), and a note says how many
findings were left out. `--report-library` shows them. A compile of the library alone reports everything, and
`report_library_macro_expansions = True` keeps the macro expansions. Other libraries (VIP) are named with
`library_paths`.

Unknown rule names produce a warning so typos do not go unnoticed.

### Path globs

* Relative paths are relative to the config file, or to the project root (the
  folder holding `.svlang-lint/`) for a config inside `.svlang-lint/`. `--library` /
  `--exclude` on the command line are relative to the working directory.
* `*` and `?` match within one folder name, `**` matches any number of folders.
* A pattern naming a folder covers everything under it (`"ip/vip"` = `"ip/vip/**"`).
* A pattern with no `/` matches that name at any depth (`"generated"`, `"*_pkg.sv"`).
* `$VAR`, `${VAR}` and a leading `~` are expanded. A pattern whose variable is
  unset is ignored with a warning, so `$UVM_HOME/**` can never turn into `/**`.

### Command line

```bash
svlang lint tb/top.sv -I $UVM_HOME/src --library '$UVM_HOME' --exclude 'tb/gen/**'
```

A waiver comment can carry the same expression in quotes. It is searched, not matched against the whole message:

```systemverilog
if (a == a) $display("one"); // svlang-lint:waive SVL-COR-004 "expression 'a'"
// svlang-lint: waive SVL-COR-004 "expression 'b'"
if (b == b) $display("two");
```

`--report-unused-waivers` prints waivers that matched nothing in this run, and waivers whose `expires` date has passed, as `SVL-WVR-002` warnings (they also count in the summary). It is off by default: one waiver file is often used for runs that lint different files, so a waiver that matched nothing this time is not always stale. `--strict-waivers` turns the warnings on and still exits 4 while any unused, expired, or stale entry remains.

### JSON form

The same settings are accepted as JSON (`.svlang-lint.json`), which is also what
the Python config is converted to internally. Comments and trailing commas are
allowed. Rules may use a shorthand: `"RTL-DAT-002": "error"`, `"MaxLineLength": "off"`.

```json
{
  "library_paths": ["$UVM_HOME", "vip/**"],
  "rules": {
    "MaxLineLength": { "options": { "max_columns": 140 } },
    "RTL-DAT-002": { "severity": "ERROR", "exclude_paths": ["legacy/**"] },
    "GenericReportId": "off"
  },
  "waivers": [
    { "rule": "UVM-OBJ-002", "path": "tb/stress_seq.sv", "line": 142,
      "reason": "Stress bench", "message": "objection" }
  ]
}
```

### Environment

* `SVLANG_LINT_PYTHON` selects the interpreter (default `python3`).
* `SVLANG_LINT_PYTHON_SDK` points at the folder holding the `svlang-lint` package if
  it is not installed and not next to the binary.
