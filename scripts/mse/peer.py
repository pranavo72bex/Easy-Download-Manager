# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

"""A libtorrent peer that makes and takes only encrypted connections (MSE,
RC4), for dm's tests (desktop/crates/dm-core/tests/mse_interop.rs):

    peer.py make <announce-url>     the torrent of /data/file.bin, as /data/t.torrent
    peer.py seed                    seeds /data/file.bin on port 6881
    peer.py leech <host> <port>     fetches it into /data/in from that peer

No DHT, local discovery or port mapping: it reaches only what it is told.
"""
import sys
import time

import libtorrent as lt

SETTINGS = {
    "listen_interfaces": "0.0.0.0:6881",
    "enable_dht": False,
    "enable_lsd": False,
    "enable_upnp": False,
    "enable_natpmp": False,
    "enable_outgoing_utp": False,
    "enable_incoming_utp": False,
    # Encrypted only, both ways, and RC4 rather than an obfuscated handshake alone.
    "out_enc_policy": int(lt.enc_policy.forced),
    "in_enc_policy": int(lt.enc_policy.forced),
    "allowed_enc_level": int(lt.enc_level.rc4),
    "prefer_rc4": True,
}


def main():
    mode = sys.argv[1]
    if mode == "make":
        files = lt.file_storage()
        lt.add_files(files, "/data/file.bin")
        torrent = lt.create_torrent(files, 64 * 1024)
        torrent.add_tracker(sys.argv[2])
        lt.set_piece_hashes(torrent, "/data")
        with open("/data/t.torrent", "wb") as out:
            out.write(lt.bencode(torrent.generate()))
        print("made", flush=True)
        return
    session = lt.session(SETTINGS)
    info = lt.torrent_info("/data/t.torrent")
    if mode == "seed":
        handle = session.add_torrent({"ti": info, "save_path": "/data"})
        while not handle.status().is_seeding:
            time.sleep(0.2)
        print("seeding", flush=True)
        while True:
            time.sleep(1)
    if mode == "leech":
        handle = session.add_torrent({"ti": info, "save_path": "/data/in"})
        peer = (sys.argv[2], int(sys.argv[3]))
        deadline = time.time() + 120
        while not handle.status().is_seeding:
            if time.time() > deadline:
                print("timed out at", handle.status().progress, flush=True)
                sys.exit(1)
            if not handle.status().num_peers:
                handle.connect_peer(peer)
            time.sleep(0.5)
        print("done", flush=True)


main()
