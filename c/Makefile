
clean: 
				rm -rf bin

build_static_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract ../target/debug/libwarp.a -lm

build_static_crypto:
				mkdir -p bin && clang -Wall -Wextra crypto.c -o bin/crypto ../target/debug/libwarp.a -lm

build_static_fs_memory:
				mkdir -p bin && clang -Wall -Wextra fs_memory.c -o bin/fs_memory ../target/debug/libwarp.a ../target/debug/libwarp_fs_memory.a -lm

build_static_fs_storj:
				mkdir -p bin && clang -Wall -Wextra fs_storj.c -o bin/fs_storj ../target/debug/libwarp.a ../target/debug/libwarp_fs_storj.a ../target/debug/libwarp_pd_flatfile.a -lm -lssl -lcrypto

build_shared_tesseract:
				mkdir -p bin && clang -Wall -Wextra tesseract.c -o bin/tesseract -L ../target/debug -lwarp -lpthread -ldl -lssl -lcrypto -lbz2 -lm -ludev

build_static_mp_solana:
				mkdir -p bin && clang -Wall -Wextra mp_solana.c -o bin/mp_solana ../target/debug/libwarp.a ../target/debug/libwarp_mp_solana.a -lz -lssl -lcrypto -lbz2 -lm -ludev

build_static_mp_ipfs:
				mkdir -p bin && clang -Wall -Wextra mp_ipfs.c -o bin/mp_ipfs ../target/debug/libwarp.a ../target/debug/libwarp_mp_ipfs.a -lz -lm

build_static_pd_flatfile:
				mkdir -p bin && clang -Wall -Wextra pd_flatfile.c -o bin/pd_flatfile ../target/debug/libwarp.a ../target/debug/libwarp_pd_flatfile.a -lm

build_shared_pd_flatfile:
				mkdir -p bin && clang -Wall -Wextra pd_flatfile.c -o bin/pd_flatfile -L ../target/debug -lwarp -lwarp_pd_flatfile -lpthread -ldl -lm

build_static_rg_libp2p:
				mkdir -p bin && clang -Wall -Wextra rg_libp2p.c -o bin/rg_libp2p ../target/debug/libwarp.a ../target/debug/libwarp_rg_libp2p.a ../target/debug/libwarp_mp_solana.a -lz -lssl -lcrypto -lbz2 -lm -ludev