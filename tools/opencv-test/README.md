# build dependencies

# testing all codecs
`cargo build --package opencv-test -F all`

## Mac OS
clang  - comes with llvm. `brew install llvm`. symlink $(brew --prefix llvm)/lib/libclang.dylib to wherever is needed
opencv

## Linux
libopencv-dev
libstdc++-12-dev
clang
libclang-dev
libx264-dev
libaom-dev

## video file extensions and codecs known to work with opencv
- .avi / MJPG
- .mkv / H264
- .mp4 / avc1

## testing a .mov file
ffmpeg -i <filename.mov> output_%04d.png