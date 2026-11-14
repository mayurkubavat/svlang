# Type stubs for the native module. Keep in step with src/lib.rs.
from os import PathLike
from typing import Mapping, Optional, Sequence, Union

__version__: str
SYNTAX_KINDS: list[str]
TOKEN_KINDS: list[str]

class SvlangError(Exception): ...

class LowerError(SvlangError):
    diagnostics: list[NetlistDiagnostic]
    unsupported: bool

class Location:
    file: str
    line: int
    column: int
    expansion_file: Optional[str]
    expansion_line: Optional[int]

class Diagnostic:
    file: str
    line: int
    column: int
    stage: str
    severity: str
    message: str

class SyntaxError:
    message: str
    location: Optional[Location]

class Token:
    @property
    def kind(self) -> str: ...
    @property
    def text(self) -> str: ...
    @property
    def leading_trivia(self) -> str: ...
    @property
    def from_macro(self) -> bool: ...
    @property
    def index(self) -> int: ...
    @property
    def location(self) -> Optional[Location]: ...

class SyntaxNode:
    @property
    def kind(self) -> str: ...
    @property
    def id(self) -> int: ...
    @property
    def tree(self) -> SyntaxTree: ...
    @property
    def children(self) -> list[Union[SyntaxNode, Token]]: ...
    @property
    def child_nodes(self) -> list[SyntaxNode]: ...
    @property
    def tokens(self) -> list[Token]: ...
    @property
    def text(self) -> str: ...
    @property
    def bare_text(self) -> str: ...
    @property
    def location(self) -> Optional[Location]: ...
    def child(self, kind: str) -> Optional[SyntaxNode]: ...
    def walk(self) -> list[SyntaxNode]: ...
    def find_all(self, kind: str) -> list[SyntaxNode]: ...

class SyntaxTree:
    @property
    def root(self) -> SyntaxNode: ...
    @property
    def files(self) -> list[str]: ...
    @property
    def errors(self) -> list[SyntaxError]: ...
    @property
    def is_library(self) -> bool: ...
    def preorder(self) -> list[SyntaxNode]: ...
    def tokens(self) -> list[Token]: ...
    def file_text(self, index: int = 0) -> str: ...
    def dump(self) -> str: ...
    def __len__(self) -> int: ...

class Signal:
    name: str
    width: int
    signed: bool

class NetlistDiagnostic:
    file: str
    line: int
    column: int
    message: str
    unsupported: bool

class Netlist:
    @property
    def top(self) -> str: ...
    @property
    def inputs(self) -> list[Signal]: ...
    @property
    def outputs(self) -> list[Signal]: ...
    @property
    def states(self) -> list[Signal]: ...
    @property
    def num_values(self) -> int: ...
    def dump(self) -> str: ...

class Compilation:
    def __init__(
        self,
        files: Sequence[Union[str, PathLike[str]]] = ...,
        *,
        include_dirs: Sequence[Union[str, PathLike[str]]] = ...,
        defines: Optional[Mapping[str, Optional[object]]] = None,
        top: Sequence[str] = ...,
        params: Optional[Mapping[str, object]] = None,
        sources: Optional[Mapping[str, str]] = None,
        single_unit: bool = False,
    ) -> None: ...
    @staticmethod
    def from_source(text: str, name: str = "source.sv") -> Compilation: ...
    @staticmethod
    def from_args(args: Sequence[str]) -> Compilation: ...
    @property
    def trees(self) -> list[SyntaxTree]: ...
    @property
    def tops(self) -> list[str]: ...
    @property
    def has_syntax_errors(self) -> bool: ...
    def diagnostics(self) -> list[Diagnostic]: ...
    def hierarchy(self) -> list[str]: ...
    def lower_netlist(
        self,
        *,
        top: Optional[str] = None,
        max_loop_iters: int = 65536,
        blast_memories_up_to: Optional[int] = None,
        allow_four_state_ops: bool = False,
        cut_points: Sequence[str] = ...,
        black_box_modules: Sequence[str] = ...,
        black_box_instances: Sequence[str] = ...,
    ) -> Netlist: ...
