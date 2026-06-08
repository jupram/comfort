#!/usr/bin/env python3
import argparse
import base64
import json
import math
import sys
import time
import urllib.request
from pathlib import Path

import cv2

HAND_CONNECTIONS = [
    (0, 1), (1, 2), (2, 3), (3, 4),
    (0, 5), (5, 6), (6, 7), (7, 8),
    (5, 9), (9, 10), (10, 11), (11, 12),
    (9, 13), (13, 14), (14, 15), (15, 16),
    (13, 17), (17, 18), (18, 19), (19, 20),
    (0, 17),
]

MODEL_DOWNLOAD_TIMEOUT_S = 30
MODEL_URL = (
    "https://storage.googleapis.com/mediapipe-models/"
    "hand_landmarker/hand_landmarker/float16/latest/hand_landmarker.task"
)


def emit(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":"), allow_nan=False) + "\n")
    sys.stdout.flush()


def clamp(value, low, high):
    return max(low, min(high, value))


def finite_float(value, default=0.0):
    try:
        value = float(value)
    except (TypeError, ValueError):
        return default
    return value if math.isfinite(value) else default


def sanitize_landmark(point):
    return {
        "x": clamp(finite_float(point.x), 0.0, 1.0),
        "y": clamp(finite_float(point.y), 0.0, 1.0),
        "z": clamp(finite_float(point.z), -1.0, 1.0),
    }


def draw_landmarks_on_frame(frame, landmarks):
    h, w = frame.shape[:2]
    points = []
    for p in landmarks:
        x = int(max(0.0, min(1.0, p["x"])) * w)
        y = int(max(0.0, min(1.0, p["y"])) * h)
        points.append((x, y))
    for a, b in HAND_CONNECTIONS:
        if a < len(points) and b < len(points):
            cv2.line(frame, points[a], points[b], (80, 255, 160), 2, cv2.LINE_AA)
    for i, (x, y) in enumerate(points):
        color = (64, 220, 255) if i in (4, 8, 12, 16, 20) else (255, 180, 80)
        cv2.circle(frame, (x, y), 3, color, -1, cv2.LINE_AA)

def make_preview_jpeg_b64(frame, max_width=640):
    preview = frame
    h, w = preview.shape[:2]
    if w > max_width:
        scale = max_width / float(w)
        preview = cv2.resize(preview, (max_width, int(h * scale)), interpolation=cv2.INTER_AREA)
    ok, encoded = cv2.imencode(".jpg", preview, [int(cv2.IMWRITE_JPEG_QUALITY), 75])
    if not ok:
        return None
    return base64.b64encode(encoded).decode("ascii")


def sleep_remaining_frame_time(frame_started, frame_sleep):
    remaining = frame_sleep - (time.monotonic() - frame_started)
    if remaining > 0:
        time.sleep(remaining)


def ensure_model(model_path):
    path = Path(model_path)
    if path.is_file() and path.stat().st_size > 0:
        return str(path)

    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    tmp_path.unlink(missing_ok=True)
    print(f"downloading model: {MODEL_URL}", file=sys.stderr, flush=True)
    try:
        with urllib.request.urlopen(MODEL_URL, timeout=MODEL_DOWNLOAD_TIMEOUT_S) as response:
            with tmp_path.open("wb") as out:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    out.write(chunk)
        if not tmp_path.is_file() or tmp_path.stat().st_size == 0:
            raise RuntimeError("downloaded model is empty")
        tmp_path.replace(path)
    except Exception:
        tmp_path.unlink(missing_ok=True)
        raise
    print(f"model saved: {path}", file=sys.stderr, flush=True)
    return str(path)


def main():
    parser = argparse.ArgumentParser(description="MediaPipe hand tracker NDJSON sidecar")
    parser.add_argument("--camera-index", type=int, default=0)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument("--preview-every", type=int, default=3)
    parser.add_argument(
        "--model-path",
        type=str,
        default="vision/models/hand_landmarker.task",
        help="Path to MediaPipe hand landmarker .task model",
    )
    args = parser.parse_args()
    args.camera_index = max(args.camera_index, 0)
    args.width = clamp(args.width, 160, 3840)
    args.height = clamp(args.height, 120, 2160)
    args.fps = clamp(args.fps, 1, 120)
    args.preview_every = max(args.preview_every, 0)

    backend_candidates = [
        ("MSMF", cv2.CAP_MSMF),
        ("DSHOW", cv2.CAP_DSHOW),
        ("ANY", cv2.CAP_ANY),
    ]
    cap = None
    opened_backend = None
    for name, backend in backend_candidates:
        trial = cv2.VideoCapture(args.camera_index, backend)
        if trial.isOpened():
            cap = trial
            opened_backend = name
            break
        trial.release()

    if cap is None:
        print(
            f"failed to open camera index={args.camera_index} on backends MSMF/DSHOW/ANY",
            file=sys.stderr,
            flush=True,
        )
        emit({"frame_id": 0, "ts_ms": int(time.time() * 1000), "confidence": 0.0, "landmarks": []})
        return 1
    print(f"camera opened: index={args.camera_index} backend={opened_backend}", file=sys.stderr, flush=True)

    hand_landmarker = None
    try:
        cap.set(cv2.CAP_PROP_FRAME_WIDTH, args.width)
        cap.set(cv2.CAP_PROP_FRAME_HEIGHT, args.height)
        cap.set(cv2.CAP_PROP_FPS, args.fps)
        cap.set(cv2.CAP_PROP_BUFFERSIZE, 1)

        model_path = ensure_model(args.model_path)

        import mediapipe as mp
        from mediapipe.tasks import python as mp_python
        from mediapipe.tasks.python import vision

        base_options = mp_python.BaseOptions(model_asset_path=model_path)
        options = vision.HandLandmarkerOptions(
            base_options=base_options,
            running_mode=vision.RunningMode.IMAGE,
            num_hands=1,
            min_hand_detection_confidence=0.5,
            min_tracking_confidence=0.5,
        )
        hand_landmarker = vision.HandLandmarker.create_from_options(options)

        frame_id = 0
        frame_sleep = 1.0 / max(args.fps, 1)

        while True:
            frame_started = time.monotonic()
            ok, frame = cap.read()
            ts_ms = int(time.time() * 1000)
            if not ok or frame is None:
                emit(
                    {
                        "frame_id": frame_id,
                        "ts_ms": ts_ms,
                        "confidence": 0.0,
                        "landmarks": [],
                        "preview_jpeg_base64": None,
                    }
                )
                frame_id += 1
                sleep_remaining_frame_time(frame_started, frame_sleep)
                continue

            rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            mp_image = mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb)
            results = hand_landmarker.detect(mp_image)

            confidence = 0.0
            landmarks = []
            if results.hand_landmarks:
                hand = results.hand_landmarks[0]
                landmarks = [sanitize_landmark(p) for p in hand]
                if len(landmarks) != 21:
                    landmarks = []
                if results.handedness and len(results.handedness) > 0:
                    confidence = clamp(finite_float(results.handedness[0][0].score), 0.0, 1.0)
                else:
                    confidence = 1.0

            preview_jpeg_base64 = None
            if args.preview_every > 0 and frame_id % args.preview_every == 0:
                preview_frame = frame
                if landmarks:
                    draw_landmarks_on_frame(preview_frame, landmarks)
                label = f"conf={confidence:.2f} frame={frame_id}"
                cv2.putText(
                    preview_frame,
                    label,
                    (12, 26),
                    cv2.FONT_HERSHEY_SIMPLEX,
                    0.7,
                    (245, 245, 245),
                    2,
                    cv2.LINE_AA,
                )
                preview_jpeg_base64 = make_preview_jpeg_b64(preview_frame)

            emit(
                {
                    "frame_id": frame_id,
                    "ts_ms": ts_ms,
                    "confidence": confidence,
                    "landmarks": landmarks,
                    "preview_jpeg_base64": preview_jpeg_base64,
                }
            )
            frame_id += 1
            sleep_remaining_frame_time(frame_started, frame_sleep)
    except KeyboardInterrupt:
        pass
    finally:
        cap.release()
        if hand_landmarker is not None:
            hand_landmarker.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
