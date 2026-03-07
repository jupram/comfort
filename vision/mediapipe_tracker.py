#!/usr/bin/env python3
import argparse
import base64
import json
import os
import sys
import time
import urllib.request

import cv2
import mediapipe as mp

HAND_CONNECTIONS = [
    (0, 1), (1, 2), (2, 3), (3, 4),
    (0, 5), (5, 6), (6, 7), (7, 8),
    (5, 9), (9, 10), (10, 11), (11, 12),
    (9, 13), (13, 14), (14, 15), (15, 16),
    (13, 17), (17, 18), (18, 19), (19, 20),
    (0, 17),
]


def emit(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()

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

    cap.set(cv2.CAP_PROP_FRAME_WIDTH, args.width)
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, args.height)
    cap.set(cv2.CAP_PROP_FPS, args.fps)

    model_path = args.model_path
    if not os.path.exists(model_path):
        model_dir = os.path.dirname(model_path)
        if model_dir:
            os.makedirs(model_dir, exist_ok=True)
        model_url = (
            "https://storage.googleapis.com/mediapipe-models/"
            "hand_landmarker/hand_landmarker/float16/latest/hand_landmarker.task"
        )
        print(f"downloading model: {model_url}", file=sys.stderr, flush=True)
        urllib.request.urlretrieve(model_url, model_path)
        print(f"model saved: {model_path}", file=sys.stderr, flush=True)

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

    try:
        while True:
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
                time.sleep(frame_sleep)
                continue

            rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            mp_image = mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb)
            results = hand_landmarker.detect(mp_image)

            confidence = 0.0
            landmarks = []
            if results.hand_landmarks:
                hand = results.hand_landmarks[0]
                landmarks = [{"x": p.x, "y": p.y, "z": p.z} for p in hand]
                if results.handedness and len(results.handedness) > 0:
                    confidence = float(results.handedness[0][0].score)
                else:
                    confidence = 1.0

            preview_jpeg_base64 = None
            if args.preview_every > 0 and frame_id % args.preview_every == 0:
                preview_frame = frame.copy()
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
    except KeyboardInterrupt:
        pass
    finally:
        cap.release()
        hand_landmarker.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
