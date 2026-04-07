#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    parser.add_argument("--beta", action="store_true")
    parser.add_argument("--char-ranges", type=int, default=0)
    parser.add_argument("--custom-model", default="")
    parser.add_argument("--custom-charset", default="")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[2]
    sys.path.insert(0, str(repo_root / "src"))

    from ticket_bot.captcha.solver import CaptchaSolver
    from ticket_bot.config import CaptchaConfig

    config = CaptchaConfig(
        engine="ddddocr",
        beta_model=args.beta,
        char_ranges=args.char_ranges,
        custom_model_path=args.custom_model,
        custom_charset_path=args.custom_charset,
    )
    solver = CaptchaSolver(config)
    image_bytes = Path(args.image).read_bytes()
    text, confidence = solver.solve(image_bytes)
    print(f"text={text}")
    print(f"confidence={confidence}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
