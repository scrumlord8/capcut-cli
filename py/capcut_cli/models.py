"""Data models for assets and compose jobs."""
from dataclasses import dataclass, field, asdict
from typing import Optional, List


@dataclass
class Asset:
    id: str
    type: str  # "sound" or "clip"
    title: str
    source_url: str
    source_platform: str
    downloaded_at: str
    duration_seconds: float
    file_path: str
    file_size_bytes: int
    format: str
    tags: List[str] = field(default_factory=list)

    def to_dict(self):
        return asdict(self)


@dataclass
class TrendingSound:
    rank: int
    title: str
    artist: str
    tiktok_url: str
    usage_count: Optional[int] = None
    trend_direction: Optional[str] = None
    duration_seconds: Optional[float] = None

    def to_dict(self):
        return asdict(self)


@dataclass
class ComposeResult:
    output_path: str
    duration_seconds: float
    file_size_bytes: int
    sound_id: str
    clip_ids: List[str]
    resolution: str

    def to_dict(self):
        return asdict(self)
