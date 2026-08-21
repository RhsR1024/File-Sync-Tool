import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("prepare-device-simulator-materials.py")
SPEC = importlib.util.spec_from_file_location("prepare_device_simulator_materials", SCRIPT_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PreparedMediaQualityProfileTests(unittest.TestCase):
    def test_three_renditions_raise_quality_without_changing_timing_contract(self):
        self.assertEqual(
            [
                (item["kind"], item["width"], item["height"], item["fps"], item["bitrate"])
                for item in MODULE.RENDITIONS
            ],
            [
                ("main", 1920, 1080, 25, 6_000_000),
                ("sub", 640, 360, 20, 1_000_000),
                ("third", 640, 360, 20, 1_000_000),
            ],
        )
        command = MODULE.build_encode_command(
            "ffmpeg", Path("source.mp4"), Path("main.h264"), MODULE.RENDITIONS[0]
        )
        self.assertEqual(command[command.index("-preset") + 1], "medium")
        self.assertIn("flags=lanczos", command[command.index("-vf") + 1])
        self.assertEqual(command[command.index("-b:v") + 1], "6000000")
        self.assertEqual(command[command.index("-maxrate") + 1], "9000000")
        self.assertEqual(command[command.index("-bufsize") + 1], "12000000")
        self.assertEqual(command[command.index("-g") + 1], "50")
        self.assertEqual(command[command.index("-keyint_min") + 1], "50")
        self.assertEqual(command[command.index("-bf") + 1], "0")
        self.assertEqual(command[command.index("-vsync") + 1], "cfr")


if __name__ == "__main__":
    unittest.main()
