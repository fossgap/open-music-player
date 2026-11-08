#!/usr/bin/env python3
"""Create explicitly labelled, original test audio and geometric cover art.

No copyrighted recordings or network downloads are used.
"""
from pathlib import Path
import argparse
import math
import struct
import subprocess
import zlib


def png(path, color, style):
    size = 400
    pixels = bytearray()
    for y in range(size):
        pixels.append(0)
        for x in range(size):
            dx, dy = x - 200, y - 200
            radius = math.hypot(dx, dy)
            if style == 0:
                shape = 85 < radius < 145 or (radius < 30)
            elif style == 1:
                shape = abs(dx + dy * 0.5) < 32 or abs(dx + dy * 0.5 - 100) < 32
            else:
                shape = y > 120 and y < 280 and (x // 40) % 2 == 0 and abs(dy) < 85 * (1 - abs(dx) / 230)
            pixels.extend(tuple(min(255, c + 65) if shape else c for c in color))
    def chunk(name, value):
        return struct.pack('!I', len(value)) + name + value + struct.pack('!I', zlib.crc32(name + value) & 0xffffffff)
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', size, size, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))


def create(destination, seconds=45):
    destination.mkdir(parents=True, exist_ok=True)
    albums = [
        ('Daylight studies', (91, 114, 77), ['Slow mornings', 'An open window', 'A place to begin', 'Still here']),
        ('After hours', (111, 65, 52), ['The long way home', 'Streetlights', 'Quiet company', 'Past midnight']),
        ('Soft signals', (62, 83, 109), ['First light', 'A little further', 'Blue room', 'Coming around']),
    ]
    for index, (album, color, names) in enumerate(albums):
        folder = destination / album
        folder.mkdir(exist_ok=True)
        png(folder / 'cover.png', color, index)
        for number, title in enumerate(names, 1):
            audio = folder / f'{number:02d} - {title}.flac'
            subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i',
                            f'sine=frequency={220 + 55 * index + 27.5 * number}:duration={seconds}',
                            '-af', 'volume=0.1,afade=t=in:d=1,afade=t=out:st=' + str(seconds-1) + ':d=1',
                            '-metadata', f'title={title}', '-metadata', 'artist=Open Music · Demo',
                            '-metadata', f'album={album}', '-metadata', f'track={number}', str(audio)], check=True)
        (folder / f'01 - {names[0]}.txt').write_text('Original demonstration tone.\n\nThis fixture tests local lyrics, artwork,\nmetadata, and playback without using\ncommercial recordings.\n')
    (destination / 'README.txt').write_text('Generated demonstration tones and geometric artwork. Not commercial recordings.\n')
    return destination


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--seconds', type=int, default=45)
    args = parser.parse_args()
    create(args.directory, args.seconds)
    print(args.directory)
