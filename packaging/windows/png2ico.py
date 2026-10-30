#!/usr/bin/env python3
"""Decode the RGBA PNG and build Windows shortcut sizes using only the stdlib."""
import pathlib
import struct
import sys
import zlib

source, target = map(pathlib.Path, sys.argv[1:])
data = source.read_bytes()
assert data[:8] == b'\x89PNG\r\n\x1a\n'
width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', data[16:29])
assert (width, height, depth, color, interlace) == (256, 256, 8, 6, 0)
compressed = bytearray()
pos = 8
while pos < len(data):
    length = struct.unpack_from('>I', data, pos)[0]
    if data[pos + 4:pos + 8] == b'IDAT':
        compressed.extend(data[pos + 8:pos + 8 + length])
    pos += length + 12
raw = zlib.decompress(compressed)
rows = []
previous = bytearray(width * 4)
for y in range(height):
    offset = y * (width * 4 + 1)
    mode = raw[offset]
    row = bytearray(raw[offset + 1:offset + 1 + width * 4])
    for x in range(len(row)):
        a = row[x - 4] if x >= 4 else 0
        b = previous[x]
        c = previous[x - 4] if x >= 4 else 0
        p = a + b - c
        pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
        predictor = a if pa <= pb and pa <= pc else b if pb <= pc else c
        row[x] = (row[x] + (0, a, b, (a + b) // 2, predictor)[mode]) & 255
    rows.append(row)
    previous = row

images = []
for size in (16, 32, 48):
    pixels = bytearray()
    mask = bytearray()
    for y in reversed(range(size)):
        mask_row = bytearray(((size + 31) // 32) * 4)
        for x in range(size):
            # Box filtering preserves the small silhouette and avoids aliasing.
            samples = [rows[sy][sx * 4:sx * 4 + 4]
                       for sy in range(y * height // size, (y + 1) * height // size)
                       for sx in range(x * width // size, (x + 1) * width // size)]
            r, g, b, a = [sum(p[c] for p in samples) // len(samples) for c in range(4)]
            pixels.extend((b, g, r, a))
            if a == 0:
                mask_row[x // 8] |= 128 >> (x % 8)
        mask.extend(mask_row)
    header = struct.pack('<IiiHHIIiiII', 40, size, size * 2, 1, 32, 0, len(pixels), 0, 0, 0, 0)
    images.append((size, header + pixels + mask))
images.append((256, data))
offset = 6 + 16 * len(images)
entries = bytearray()
for size, payload in images:
    entries.extend(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(payload), offset))
    offset += len(payload)
target.write_bytes(struct.pack('<HHH', 0, 1, len(images)) + entries + b''.join(p for _, p in images))
