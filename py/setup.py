from setuptools import setup, find_packages

setup(
    name="capcut-cli",
    version="0.1.0",
    packages=find_packages(),
    install_requires=[
        "click==8.1.8",
        "httpx>=0.24,<1.0",
        "beautifulsoup4>=4.12",
        "imageio-ffmpeg>=0.5.1",
    ],
    entry_points={
        "console_scripts": [
            "capcut-cli=capcut_cli.cli:main",
        ],
    },
    python_requires=">=3.9",
)
