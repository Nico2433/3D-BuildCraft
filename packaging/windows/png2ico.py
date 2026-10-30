#!/usr/bin/env python3
"""Wrap the app's 256x256 PNG in an ICO without image dependencies."""
import pathlib
import struct
import sys

source = pathlib.Path(sys.argv[1])
target = pathlib.Path(sys.argv[2])
data = source.read_bytes()
if data[:8] != b'\x89PNG\r\n\x1a\n' or struct.unpack('>II', data[16:24]) != (256, 256):
    raise SystemExit('Expected a 256x256 PNG')
target.write_bytes(struct.pack('<HHH', 0, 1, 1) +
                   struct.pack('<BBBBHHII', 0, 0, 0, 0, 1, 32, len(data), 22) + data)
