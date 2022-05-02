
clean: 
				rm -rf bin

build_static_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract ../target/debug/libwarp.a -lssl -lcrypto -lbz2 -lm -ludev

build_shared_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract -L ../target/debug -lwarp -lpthread -ldl -lssl -lcrypto -lbz2 -lm -ludev