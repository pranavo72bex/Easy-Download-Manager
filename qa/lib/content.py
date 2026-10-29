"""Deterministic file content, shared by the origin server and the tests.

The byte at offset `o` of file `seed` depends only on (seed, o), so any range
can be served without a file on disk and the expected SHA-256 of any size is
known to the tests. 64 KiB blocks: a base block from the seed, rotated, and
each block XORed with its own index so no two blocks repeat within 16 MiB.
"""
import hashlib

BLOCK = 65536
_TABLES = [bytes(b ^ k for b in range(256)) for k in range(256)]
_BASE = {}


def _base(seed):
    if seed not in _BASE:
        out = bytearray()
        counter = 0
        while len(out) < BLOCK:
            out += hashlib.sha256(f"{seed}:{counter}".encode()).digest()
            counter += 1
        _BASE[seed] = bytes(out[:BLOCK])
    return _BASE[seed]


def block(seed, index):
    """The `index`-th 64 KiB block of file `seed`."""
    return _base(seed).translate(_TABLES[index & 0xFF] if (index >> 8) & 1 == 0 else _TABLES[(index * 7 + 1) & 0xFF])


def chunks(seed, start, end):
    """The bytes [start, end) of file `seed`, in pieces of at most 64 KiB."""
    pos = start
    while pos < end:
        index, offset = divmod(pos, BLOCK)
        take = min(BLOCK - offset, end - pos)
        yield block(seed, index)[offset:offset + take]
        pos += take


def data(seed, start, end):
    return b"".join(chunks(seed, start, end))


def sha256(seed, size):
    h = hashlib.sha256()
    for piece in chunks(seed, 0, size):
        h.update(piece)
    return h.hexdigest()


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for piece in iter(lambda: f.read(1 << 20), b""):
            h.update(piece)
    return h.hexdigest()
