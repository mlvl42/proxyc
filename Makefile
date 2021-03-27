test:
	echo "YO" | LD_PRELOAD=./target/debug/libproxyc.so nc -c 127.0.0.1 8080

test_noob:
	#cargo build
	cp ./target/debug/libproxyc.so ./target/debug/libgreet.so
	gcc -Wall -shared badgreet.c -o libgreet.so
	gcc -Wall main.c -lgreet -L ./
	LD_LIBRARY_PATH=. LD_PRELOAD=./target/debug/libproxyc.so ./a.out

clean:
	rm -f *.o
	rm -f *.so
	rm -f a.out
