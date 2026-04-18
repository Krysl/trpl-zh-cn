#!/usr/bin/env python3
import json
import re
import sys
from typing import Any

INLINE_BREAK = re.compile(r'(?m)^(>\s*\[[^\]]+\]\([^)]+\))\s*<br\s*/?>\s*$')
BLOCK_BREAK = re.compile(r'(?m)^>\s*<br\s*/?>\s*$')


def sanitize_markdown(text: str) -> str:
    text = INLINE_BREAK.sub(r'\1\n>', text)
    text = BLOCK_BREAK.sub('>', text)
    return text


def walk_book(node: Any) -> None:
    if isinstance(node, dict):
        if 'Chapter' in node and isinstance(node['Chapter'], dict):
            chapter = node['Chapter']
            content = chapter.get('content')
            if isinstance(content, str):
                chapter['content'] = sanitize_markdown(content)
            walk_book(chapter.get('sub_items', []))
        else:
            for value in node.values():
                walk_book(value)
    elif isinstance(node, list):
        for item in node:
            walk_book(item)


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == 'supports':
        renderer = sys.argv[2] if len(sys.argv) > 2 else ''
        sys.exit(0 if renderer == 'epub' else 1)

    raw = sys.stdin.buffer.read()
    _context, book = json.loads(raw.decode('utf-8-sig'))
    walk_book(book)
    sys.stdout.buffer.write(json.dumps(book, ensure_ascii=False).encode('utf-8'))
