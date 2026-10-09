"""Static analysis of Lua patterns (lstrlib syntax) for lua2rs.

Used to type the parameters of ModParser closures:
  * Lua patterns have no alternation and no optional groups, so on a successful match
    every capture is set. A closure parameter bound to capture k (k <= #captures) is
    therefore never nil; one bound beyond the last capture is always nil.
  * modTagList closures receive `tonumber(cap1)` when `cap1:match("%d+")`, else `cap1`
    itself (ModParser.lua:6698-6702). `first_capture_digits` decides statically which.
"""

# Character classes (lower case; upper case is the complement).
_CLASS_DIGITS = {"d": "always", "D": "never", "a": "never", "l": "never", "u": "never",
                 "s": "never", "p": "never", "c": "never", "w": "maybe", "x": "maybe",
                 "A": "maybe", "L": "maybe", "U": "maybe", "S": "maybe", "P": "maybe",
                 "C": "maybe", "W": "maybe", "X": "maybe", "g": "maybe", "G": "maybe"}


class Item:
    __slots__ = ("kind", "text", "quant")

    def __init__(self, kind, text, quant=""):
        self.kind = kind    # 'char' 'class' 'set' 'any' 'open' 'close' 'pos' 'other'
        self.text = text
        self.quant = quant  # '' '+' '*' '-' '?'

    def __repr__(self):
        return "%s(%r%s)" % (self.kind, self.text, self.quant)


def tokenize(pat):
    """Split a Lua pattern into items. Raises ValueError on malformed patterns."""
    items = []
    i = 0
    n = len(pat)
    if pat.startswith("^"):
        i = 1
    while i < n:
        c = pat[i]
        if c == "(":
            if i + 1 < n and pat[i + 1] == ")":
                items.append(Item("pos", "()"))
                i += 2
                continue
            items.append(Item("open", "("))
            i += 1
            continue
        if c == ")":
            items.append(Item("close", ")"))
            i += 1
            continue
        if c == "$" and i == n - 1:
            i += 1
            continue
        if c == "%":
            if i + 1 >= n:
                raise ValueError("malformed pattern (ends with '%')")
            d = pat[i + 1]
            if d == "b":
                items.append(Item("other", pat[i:i + 4]))
                i += 4
                continue
            if d == "f":
                j = pat.index("]", i + 3)
                items.append(Item("other", pat[i:j + 1]))
                i = j + 1
                continue
            if d.isdigit():
                items.append(Item("other", pat[i:i + 2]))
                i += 2
                continue
            it = Item("class" if d.isalpha() else "char", d)
            i += 2
        elif c == "[":
            j = i + 1
            if j < n and pat[j] == "^":
                j += 1
            if j < n and pat[j] == "]":
                j += 1
            while j < n and pat[j] != "]":
                j += 2 if pat[j] == "%" else 1
            if j >= n:
                raise ValueError("malformed pattern (missing ']')")
            it = Item("set", pat[i:j + 1])
            i = j + 1
        elif c == ".":
            it = Item("any", ".")
            i += 1
        else:
            it = Item("char", c)
            i += 1
        if i < n and pat[i] in "+*-?":
            it.quant = pat[i]
            i += 1
        items.append(it)
    return items


def captures(pat):
    """List of captures: each a list of the items inside it (position captures are the
    string 'pos'). Nested captures are listed in order of their opening parenthesis."""
    items = tokenize(pat)
    caps = []
    stack = []
    for it in items:
        if it.kind == "open":
            caps.append([])
            stack.append(len(caps) - 1)
        elif it.kind == "close":
            if not stack:
                raise ValueError("invalid pattern capture")
            stack.pop()
        elif it.kind == "pos":
            caps.append("pos")
        else:
            for k in stack:
                caps[k].append(it)
    if stack:
        raise ValueError("unfinished capture")
    return caps


def capture_count(pat):
    return len(captures(pat))


def _item_digits(it):
    """'always' if the item only matches digits, 'never' if it never matches one, else 'maybe'."""
    if it.kind == "char":
        return "always" if it.text.isdigit() else "never"
    if it.kind == "class":
        return _CLASS_DIGITS.get(it.text, "maybe")
    if it.kind == "set":
        body = it.text[1:-1]
        neg = body.startswith("^")
        if neg:
            body = body[1:]
        kinds = set()
        j = 0
        while j < len(body):
            if body[j] == "%" and j + 1 < len(body):
                d = body[j + 1]
                kinds.add(_CLASS_DIGITS.get(d, "maybe") if d.isalpha() else ("always" if d.isdigit() else "never"))
                j += 2
            elif j + 2 < len(body) and body[j + 1] == "-":
                lo, hi = body[j], body[j + 2]
                kinds.add("maybe" if lo <= "9" and hi >= "0" else "never")
                j += 3
            else:
                kinds.add("always" if body[j].isdigit() else "never")
                j += 1
        if neg:
            # [^%d] never matches a digit; any other complement may.
            return "never" if kinds == {"always"} else "maybe"
        if kinds == {"never"}:
            return "never"
        if kinds == {"always"}:
            return "always"
        return "maybe"
    return "maybe"


def first_capture_digits(pat):
    """For modTagList closures: does capture 1 contain a digit on every match?
    Returns 'num' (always: the closure gets tonumber(cap1)), 'str' (never: it gets cap1),
    'mixed' (depends on the input), or 'none' (no capture)."""
    caps = captures(pat)
    if not caps:
        return "none"
    c = caps[0]
    if c == "pos":
        return "mixed"
    kinds = [_item_digits(it) for it in c]
    # A mandatory always-digit item guarantees a digit.
    for it, k in zip(c, kinds):
        if k == "always" and it.quant in ("", "+"):
            return "num"
    if all(k == "never" for k in kinds):
        return "str"
    return "mixed"
