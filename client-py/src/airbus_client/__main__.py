from airbus_client.server import start_server


def main() -> None:
    with start_server("127.0.0.1:0") as rpc:
        print(rpc.ping())


if __name__ == "__main__":
    main()
