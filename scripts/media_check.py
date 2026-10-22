"""Whether a downloaded video or sound file is whole, read from its own
structure, with nothing but Python's standard library.

Used by out-of-box-check.py and real-sites.py to judge what dm made. It
decodes no pictures and no sound: it checks that the container is what it
says and that nothing is cut short, which is how a download goes wrong.

MP4 (and M4A):
  - every top-level box fits inside the file (a cut file ends mid-box);
  - there is an ftyp, a moov and media data (mdat);
  - mvhd gives the length (duration / timescale);
  - each trak's hdlr says what it holds ('vide' picture, 'soun' sound), its
    stsd names the codec and its tkhd the picture's size;
  - every chunk the sample tables point at (stco/co64 offsets, with the
    sizes stsz and stsc give each chunk) lies inside an mdat that is in the
    file, so the last sample's bytes are really there. A fragmented file
    (moof) is held to the same thing: each trun's samples lie inside the
    file.

WebM / Matroska:
  - the EBML header is there and its DocType is webm or matroska;
  - the Segment and every element in it with a known size end inside the
    file, and a Segment of known size is all there;
  - Info gives the length, Tracks says what each track holds (TrackType 1
    picture, 2 sound), its CodecID and PixelHeight;
  - there are Clusters, the last one's blocks are whole, and the last
    block's time reaches the stated length (within a second or 5%), so the
    file was not cut at a cluster boundary either.

    python3 scripts/media_check.py FILE...   # prints what it found
"""

import struct
import sys

__all__ = ["inspect", "summary"]


def inspect(path):
    """{'container', 'duration' (seconds or None), 'tracks': [{'kind',
    'codec', 'height'}], 'problems': [...]}. Whole when problems is empty."""
    info = {"container": None, "duration": None, "tracks": [], "problems": []}
    try:
        with open(path, "rb") as f:
            f.seek(0, 2)
            size = f.tell()
            f.seek(0)
            head = f.read(12)
            if head[:4] == b"\x1a\x45\xdf\xa3":
                _matroska(f, size, info)
            elif len(head) >= 8 and head[4:8] in (b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"styp"):
                _mp4(f, size, info)
            else:
                info["problems"].append("neither an MP4 nor a WebM/Matroska file")
    except OSError as e:
        info["problems"].append(f"could not be read: {e}")
    except EOFError:
        info["problems"].append("cut short")
    except (struct.error, ValueError, IndexError) as e:
        info["problems"].append(f"malformed: {e}")
    return info


def summary(info):
    """One line: container, length, and each track."""
    tracks = ", ".join(
        f"{t['kind']} {t['codec'] or '?'}" + (f" {t['height']}p" if t.get("height") else "")
        for t in info["tracks"]) or "no tracks"
    length = f"{info['duration']:.1f} s" if info["duration"] is not None else "length unknown"
    return f"{info['container'] or '?'}, {length}, {tracks}"


# ------------------------------------------------------------------ MP4


def _boxes(data, start=0, end=None):
    """(type, payload start, payload end) for each box in data[start:end];
    a box that runs past the end raises ValueError."""
    end = len(data) if end is None else end
    at = start
    while at + 8 <= end:
        size, kind = struct.unpack(">I4s", data[at:at + 8])
        head = 8
        if size == 1:
            size = struct.unpack(">Q", data[at + 8:at + 16])[0]
            head = 16
        elif size == 0:
            size = end - at
        if size < head or at + size > end:
            raise ValueError(f"box {kind!r} runs past its parent")
        yield kind.decode("latin-1"), at + head, at + size
        at += size


def _child(data, start, end, *path):
    for kind, s, e in _boxes(data, start, end):
        if kind == path[0]:
            return (s, e) if len(path) == 1 else _child(data, s, e, *path[1:])
    return None


def _mp4(f, size, info):
    info["container"] = "mp4"
    top = []  # (type, file offset of payload, payload end)
    at = 0
    while at < size:
        f.seek(at)
        head = f.read(16)
        if len(head) < 8:
            info["problems"].append(f"{size - at} stray bytes at the end")
            break
        box, kind = struct.unpack(">I4s", head[:8])
        hlen = 8
        if box == 1:
            if len(head) < 16:
                info["problems"].append("cut short in a box header")
                break
            box, hlen = struct.unpack(">Q", head[8:16])[0], 16
        elif box == 0:
            box = size - at
        if box < hlen:
            info["problems"].append(f"a box of impossible size {box} at {at}")
            break
        if at + box > size:
            info["problems"].append(
                f"cut short: '{kind.decode('latin-1')}' at {at} needs {at + box - size} more bytes")
            top.append((kind.decode("latin-1"), at + hlen, size))
            break
        top.append((kind.decode("latin-1"), at + hlen, at + box))
        at += box
    kinds = [k for k, _, _ in top]
    for need in ("ftyp", "moov"):
        if need not in kinds:
            info["problems"].append(f"no {need}")
    if "mdat" not in kinds:
        info["problems"].append("no media data (mdat)")
    mdats = [(s, e) for k, s, e in top if k == "mdat"]
    moov = next(((s, e) for k, s, e in top if k == "moov"), None)
    if not moov:
        return
    f.seek(moov[0])
    data = f.read(moov[1] - moov[0])
    mvhd = _child(data, 0, len(data), "mvhd")
    if mvhd:
        s = mvhd[0]
        if data[s] == 1:
            scale, length = struct.unpack(">IQ", data[s + 20:s + 32])
        else:
            scale, length = struct.unpack(">II", data[s + 12:s + 20])
        if scale:
            info["duration"] = length / scale
    else:
        info["problems"].append("no mvhd")
    timescales = {}
    for kind, s, e in _boxes(data):
        if kind != "trak":
            continue
        track = _mp4_track(data, s, e, mdats, size, info)
        info["tracks"].append({k: track[k] for k in ("kind", "codec", "height")})
        timescales[track["id"]] = track["timescale"]
    fragments = [(s, e) for k, s, e in top if k == "moof"]
    if fragments:
        _mp4_fragments(f, top, size, timescales, info)
    elif not any(t["kind"] in ("video", "audio") for t in info["tracks"]):
        info["problems"].append("no picture or sound track")


def _mp4_track(data, s, e, mdats, size, info):
    track = {"kind": "other", "codec": None, "height": None, "id": None, "timescale": None}
    tkhd = _child(data, s, e, "tkhd")
    if tkhd:
        ts, te = tkhd
        track["id"] = struct.unpack(">I", data[ts + (20 if data[ts] == 1 else 12):][:4])[0]
        height = struct.unpack(">I", data[te - 4:te])[0] >> 16
        track["height"] = height or None
    mdhd = _child(data, s, e, "mdia", "mdhd")
    if mdhd:
        ms = mdhd[0]
        track["timescale"] = struct.unpack(">I", data[ms + (20 if data[ms] == 1 else 12):][:4])[0]
    hdlr = _child(data, s, e, "mdia", "hdlr")
    if hdlr:
        handler = data[hdlr[0] + 8:hdlr[0] + 12]
        track["kind"] = {b"vide": "video", b"soun": "audio"}.get(handler, handler.decode("latin-1"))
    if track["kind"] != "video":
        track["height"] = None
    stbl = _child(data, s, e, "mdia", "minf", "stbl")
    if not stbl:
        info["problems"].append(f"a {track['kind']} track with no sample table")
        return track
    stsd = _child(data, *stbl, "stsd")
    if stsd and struct.unpack(">I", data[stsd[0] + 4:stsd[0] + 8])[0]:
        track["codec"] = data[stsd[0] + 12:stsd[0] + 16].decode("latin-1")
    # Sizes of every sample (stsz), chunk offsets (stco or co64), and how
    # many samples each chunk holds (stsc): together, where every byte of
    # this track must be.
    sizes = []
    stsz = _child(data, *stbl, "stsz")
    if stsz:
        p = stsz[0]
        fixed, count = struct.unpack(">II", data[p + 4:p + 12])
        sizes = [fixed] * count if fixed else list(struct.unpack(f">{count}I", data[p + 12:p + 12 + 4 * count]))
    offsets = []
    stco = _child(data, *stbl, "stco")
    co64 = _child(data, *stbl, "co64")
    if stco:
        n = struct.unpack(">I", data[stco[0] + 4:stco[0] + 8])[0]
        offsets = list(struct.unpack(f">{n}I", data[stco[0] + 8:stco[0] + 8 + 4 * n]))
    elif co64:
        n = struct.unpack(">I", data[co64[0] + 4:co64[0] + 8])[0]
        offsets = list(struct.unpack(f">{n}Q", data[co64[0] + 8:co64[0] + 8 + 8 * n]))
    runs = []
    stsc = _child(data, *stbl, "stsc")
    if stsc:
        n = struct.unpack(">I", data[stsc[0] + 4:stsc[0] + 8])[0]
        for i in range(n):
            first, per, _ = struct.unpack(">III", data[stsc[0] + 8 + 12 * i:stsc[0] + 20 + 12 * i])
            runs.append((first, per))
    if not sizes:
        return track  # an empty track, as in a fragmented file's moov
    if not offsets or not runs:
        info["problems"].append(f"{track['kind']} track: samples with no chunk table")
        return track
    sample = 0
    for chunk, offset in enumerate(offsets, start=1):
        per = next((p for first, p in reversed(runs) if first <= chunk), runs[0][1])
        length = sum(sizes[sample:sample + per])
        sample += per
        if not any(ms <= offset and offset + length <= me for ms, me in mdats):
            short = offset + length - size
            info["problems"].append(
                f"{track['kind']} track: chunk {chunk} of {len(offsets)} is not in the file"
                + (f" ({short} bytes missing)" if short > 0 else ""))
            break
    if sample < len(sizes):
        info["problems"].append(f"{track['kind']} track: {len(sizes) - sample} samples in no chunk")
    return track


def _mp4_fragments(f, top, size, timescales, info):
    """Each moof's truns: their samples must lie inside the file. The
    length, where mvhd leaves it at 0, is summed from the sample durations."""
    totals = {}
    for kind, s, e in top:
        if kind != "moof":
            continue
        f.seek(s)
        data = f.read(e - s)
        moof_start = s - 8
        for tkind, ts, te in _boxes(data):
            if tkind != "traf":
                continue
            tfhd = _child(data, ts, te, "tfhd")
            if not tfhd:
                continue
            p = tfhd[0]
            flags = int.from_bytes(data[p + 1:p + 4], "big")
            track_id = struct.unpack(">I", data[p + 4:p + 8])[0]
            p += 8
            base = moof_start
            if flags & 0x1:
                base = struct.unpack(">Q", data[p:p + 8])[0]
                p += 8
            if flags & 0x2:
                p += 4
            default_duration = default_size = 0
            if flags & 0x8:
                default_duration = struct.unpack(">I", data[p:p + 4])[0]
                p += 4
            if flags & 0x10:
                default_size = struct.unpack(">I", data[p:p + 4])[0]
            for rkind, rs, _ in _boxes(data, ts, te):
                if rkind != "trun":
                    continue
                rflags = int.from_bytes(data[rs + 1:rs + 4], "big")
                count = struct.unpack(">I", data[rs + 4:rs + 8])[0]
                q = rs + 8
                offset = base
                if rflags & 0x1:
                    offset = base + struct.unpack(">i", data[q:q + 4])[0]
                    q += 4
                if rflags & 0x4:
                    q += 4
                length = duration = 0
                for _ in range(count):
                    d = default_duration
                    if rflags & 0x100:
                        d = struct.unpack(">I", data[q:q + 4])[0]
                        q += 4
                    n = default_size
                    if rflags & 0x200:
                        n = struct.unpack(">I", data[q:q + 4])[0]
                        q += 4
                    q += 4 * bool(rflags & 0x400) + 4 * bool(rflags & 0x800)
                    length += n
                    duration += d
                if offset + length > size:
                    info["problems"].append(f"cut short: a fragment's samples need {offset + length - size} more bytes")
                totals[track_id] = totals.get(track_id, 0) + duration
    if not info["duration"]:
        lengths = [t / timescales[i] for i, t in totals.items() if timescales.get(i)]
        info["duration"] = max(lengths) if lengths else None


# ----------------------------------------------------------- Matroska

UNKNOWN = -1
SEGMENT, INFO, TRACKS, CLUSTER = 0x18538067, 0x1549A966, 0x1654AE6B, 0x1F43B675
TOP_LEVEL = {SEGMENT, INFO, TRACKS, CLUSTER, 0x114D9B74, 0x1C53BB6B, 0x1941A469, 0x1043A770, 0x1254C367}


def _vint(f, keep_marker):
    first = f.read(1)
    if not first:
        raise EOFError
    b = first[0]
    length = 1
    while length <= 8 and not b & (0x80 >> (length - 1)):
        length += 1
    if length > 8:
        raise ValueError("an EBML number longer than 8 bytes")
    rest = f.read(length - 1)
    if len(rest) < length - 1:
        raise EOFError
    value = b if keep_marker else b & (0xFF >> length)
    for c in rest:
        value = (value << 8) | c
    if not keep_marker and value == (1 << (7 * length)) - 1:
        return UNKNOWN, length
    return value, length


def _element(f):
    """(id, size or UNKNOWN, data start); EOFError at the end of the file."""
    eid, _ = _vint(f, True)
    size, _ = _vint(f, False)
    return eid, size, f.tell()


def _uint(f, n):
    return int.from_bytes(f.read(n), "big")


def _matroska(f, size, info):
    f.seek(0)
    eid, hsize, start = _element(f)
    header_end = start + hsize
    if hsize == UNKNOWN or header_end > size:
        info["problems"].append("EBML header cut short")
        return
    doctype = None
    while f.tell() < header_end:
        cid, csize, cstart = _element(f)
        if cid == 0x4282:
            doctype = f.read(csize).decode("ascii", "replace")
        f.seek(cstart + csize)
    info["container"] = doctype
    if doctype not in ("webm", "matroska"):
        info["problems"].append(f"DocType is {doctype!r}, not webm or matroska")
    f.seek(header_end)
    try:
        eid, ssize, seg = _element(f)
    except EOFError:
        info["problems"].append("no Segment")
        return
    if eid != SEGMENT:
        info["problems"].append("no Segment after the EBML header")
        return
    seg_end = size if ssize == UNKNOWN else seg + ssize
    if seg_end > size:
        info["problems"].append(f"cut short: the Segment needs {seg_end - size} more bytes")
        seg_end = size
    scale, stated = 1_000_000, None
    clusters = []  # (start of data, size or UNKNOWN)
    at = seg
    while at < seg_end:
        f.seek(at)
        try:
            cid, csize, cstart = _element(f)
        except EOFError:
            info["problems"].append("cut short in an element header")
            break
        if csize == UNKNOWN:
            if cid != CLUSTER:
                info["problems"].append(f"element {cid:#x} of unknown size")
                break
            clusters.append((cstart, UNKNOWN))
            at = _cluster_end(f, cstart, seg_end, info)
            continue
        if cstart + csize > seg_end:
            info["problems"].append(f"cut short: element {cid:#x} at {at} needs {cstart + csize - seg_end} more bytes")
            if cid == CLUSTER:
                clusters.append((cstart, csize))
            break
        if cid == INFO:
            scale, stated = _info(f, cstart, cstart + csize, scale)
        elif cid == TRACKS:
            _tracks(f, cstart, cstart + csize, info)
        elif cid == CLUSTER:
            clusters.append((cstart, csize))
        at = cstart + csize
    if stated is not None:
        info["duration"] = stated * scale / 1e9
    if not info["tracks"]:
        info["problems"].append("no Tracks")
    if not clusters:
        info["problems"].append("no Clusters")
        return
    last_start, last_size = clusters[-1]
    end = seg_end if last_size == UNKNOWN else min(last_start + last_size, seg_end)
    last_time = _last_block_time(f, last_start, end, info)
    if last_time is not None and info["duration"]:
        reached = last_time * scale / 1e9
        if reached < info["duration"] - max(1.0, 0.05 * info["duration"]):
            info["problems"].append(f"cut short: the last block is at {reached:.1f} s of {info['duration']:.1f} s")


def _cluster_end(f, start, limit, info):
    """Where a Cluster of unknown size ends: at the next top-level element."""
    at = start
    while at < limit:
        f.seek(at)
        try:
            cid, csize, cstart = _element(f)
        except EOFError:
            info["problems"].append("cut short inside a Cluster")
            return limit
        if cid in TOP_LEVEL:
            return at
        if csize == UNKNOWN or cstart + csize > limit:
            info["problems"].append("cut short: a block runs past the end of the file")
            return limit
        at = cstart + csize
    return at


def _info(f, start, end, scale):
    stated = None
    f.seek(start)
    while f.tell() < end:
        cid, csize, cstart = _element(f)
        if cid == 0x2AD7B1:
            scale = _uint(f, csize)
        elif cid == 0x4489:
            raw = f.read(csize)
            stated = struct.unpack(">f" if csize == 4 else ">d", raw)[0]
        f.seek(cstart + csize)
    return scale, stated


def _tracks(f, start, end, info):
    f.seek(start)
    while f.tell() < end:
        cid, csize, cstart = _element(f)
        if cid == 0xAE:
            track = {"kind": "other", "codec": None, "height": None}
            while f.tell() < cstart + csize:
                tid, tsize, tstart = _element(f)
                if tid == 0x83:
                    track["kind"] = {1: "video", 2: "audio", 17: "subtitle"}.get(_uint(f, tsize), "other")
                elif tid == 0x86:
                    track["codec"] = f.read(tsize).decode("ascii", "replace")
                elif tid == 0xE0:
                    while f.tell() < tstart + tsize:
                        vid, vsize, vstart = _element(f)
                        if vid == 0xBA:
                            track["height"] = _uint(f, vsize)
                        f.seek(vstart + vsize)
                f.seek(tstart + tsize)
            info["tracks"].append(track)
        f.seek(cstart + csize)


def _last_block_time(f, start, end, info):
    """The time (in the segment's ticks) of the last block of the Cluster
    at start, every block of which must be whole."""
    base, last = 0, None
    at = start
    while at < end:
        f.seek(at)
        try:
            cid, csize, cstart = _element(f)
        except EOFError:
            break
        if cid in TOP_LEVEL:
            break
        if csize == UNKNOWN or cstart + csize > end:
            info["problems"].append("cut short: the last Cluster's last block is not whole")
            break
        if cid == 0xE7:
            base = _uint(f, csize)
        elif cid in (0xA3, 0xA0):
            if cid == 0xA0:  # a BlockGroup: its Block (0xA1) holds the time
                while f.tell() < cstart + csize:
                    bid, bsize, bstart = _element(f)
                    if bid == 0xA1:
                        break
                    f.seek(bstart + bsize)
            _vint(f, False)  # the track number
            last = base + struct.unpack(">h", f.read(2))[0]
        at = cstart + csize
    return last


if __name__ == "__main__":
    bad = 0
    for name in sys.argv[1:]:
        result = inspect(name)
        print(f"{name}: {summary(result)}")
        for problem in result["problems"]:
            print(f"  {problem}")
        bad += bool(result["problems"])
    sys.exit(1 if bad else 0)
