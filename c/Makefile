
clean: 
				rm -rf bin

build_static_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract ../target/debug/libwarp.a -lssl -lcrypto -lbz2 -lm -ludev

build_shared_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract -L ../target/debug -lwarp -lpthread -ldl -lssl -lcrypto -lbz2 -lm -ludev

build_static_mp_solana:
				mkdir -p bin && clang -Wall -Wextra mp_solana.c -o bin/mp_solana ../target/debug/libwarp.a ../target/debug/libwarp_mp_solana.a -lssl -lcrypto -lbz2 -lm -ludev