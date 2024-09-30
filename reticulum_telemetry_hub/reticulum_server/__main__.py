"""
Reticulum Telemetry Hub (RTH) - Main Execution Script

This code  initializes and manages the Reticulum Telemetry Hub (RTH) as part of the RNS ecosystem.
The hub is designed to provide TCP node functionalities, to handle telemetry data collection, message routing, and peer communication within a Reticulum network.

Key Components:
- **TelemetryController**: Manages telemetry data and command handling.
- **AnnounceHandler**: Listens for and processes announcements from other nodes in the Reticulum network.
- **LXMF Router**: Manages message delivery, routing, and storage within the LXMF protocol.

Functionalities:
- **One to Many Messages**: Broadcasts messages to all connected clients (experimental).
- **Telemetry Collector**: Stores telemetry data from connected clients (currently supporting Sideband).
- **Replication Node**: Saves messages for later delivery if the recipient is offline.
- **Reticulum Transport**: Routes traffic, passes network announcements, and handles path requests.

Usage:
- The script loads or generates an identity for the hub, configures the LXMF router, and starts the main loop.
- It supports manual commands to announce the hub identity or request telemetry from a specific connection.

Configuration:
- Modify `STORAGE_PATH` and `IDENTITY_PATH` to change where data is stored.
- The `APP_NAME` constant defines the application name used in LXMF.

Running the Script:
- Execute this script directly to start the hub and enter the interactive command loop.
- Commands include `exit` to terminate, `announce` to re-announce the hub identity, and `telemetry` to request telemetry from a connected peer.

Author: FreeTAKTeam
Date: Aug 2024 
"""

import os
import time
import LXMF
import RNS
import argparse
from pathlib import Path
from reticulum_telemetry_hub.lxmf_telemetry.telemetry_controller import (
    TelemetryController,
)

# Constants
STORAGE_PATH = "RTH_Store"  # Path to store temporary files
IDENTITY_PATH = os.path.join(STORAGE_PATH, "identity")  # Path to store identity file
APP_NAME = LXMF.APP_NAME + ".delivery"  # Application name for LXMF
PLUGIN_COMMAND = (
    0  # Command to join the network, equivalent to ping on the sideband client
)


class AnnounceHandler:
    """Handles announcements from other nodes in the Reticulum network."""

    def __init__(self, identities):
        self.aspect_filter = APP_NAME  # Filter for LXMF announcements
        self.identities = identities  # Dictionary to store identities

    def received_announce(self, destination_hash, announced_identity, app_data):
        # Log the received announcement details
        RNS.log("\t+--- LXMF Announcement -----------------------------------------")
        RNS.log(f"\t| Source hash            : {RNS.prettyhexrep(destination_hash)}")
        RNS.log(f"\t| Announced identity     : {announced_identity}")
        RNS.log(f"\t| App data               : {app_data}")
        RNS.log("\t+---------------------------------------------------------------")
        self.identities[destination_hash] = app_data.decode("utf-8")


class ReticulumTelemetryHub:
    """Reticulum Telemetry Hub (RTH)"""

    lxm_router: LXMF.LXMRouter
    connections: dict[bytes, RNS.Destination]
    identities: dict[str, str]
    my_lxmf_dest: RNS.Destination
    ret: RNS.Reticulum
    storage_path: Path
    identity_path: Path
    tel_controller: TelemetryController

    def __init__(self, display_name: str, storage_path: Path, identity_path: Path):
        self.ret = RNS.Reticulum()  # Initialize Reticulum
        self.tel_controller = TelemetryController()  # Initialize telemetry controller
        self.connections = {}  # List to store connections

        identity = self.load_or_generate_identity(
            identity_path
        )  # Load or generate identity

        self.lxm_router = LXMF.LXMRouter(
            storagepath=storage_path
        )  # Initialize LXMF router

        self.my_lxmf_dest = self.lxm_router.register_delivery_identity(
            identity, display_name=display_name
        )  # Register delivery identity

        self.identities = {}  # Dictionary to store identities

        self.lxm_router.set_message_storage_limit(megabytes=5)

        # Register delivery callback
        self.lxm_router.register_delivery_callback(
            lambda msg: self.delivery_callback(msg)
        )

        # Register announce handler
        RNS.Transport.register_announce_handler(
            AnnounceHandler(self.identities)
        )

    def command_handler(self, commands: list, message: LXMF.LXMessage):
        """Handles commands received from the client and sends responses back.

        Args:
            commands (list): List of commands received from the client
            message (LXMF.LXMessage): LXMF message object
        """
        for command in commands:
            print(f"Command: {command}")
            if PLUGIN_COMMAND in command and command[PLUGIN_COMMAND] == "join":
                dest = RNS.Destination(
                    message.source.identity,
                    RNS.Destination.OUT,
                    RNS.Destination.SINGLE,
                    "lxmf",
                    "delivery",
                )
                self.connections[dest.identity.hash] = dest
                RNS.log(f"Connection added: {message.source}")
                confirmation = LXMF.LXMessage(
                    dest,
                    self.my_lxmf_dest,
                    "Connection established",
                    desired_method=LXMF.LXMessage.DIRECT,
                )
                self.lxm_router.handle_outbound(confirmation)
                continue  # Skip the rest of the loop
            elif PLUGIN_COMMAND in command and command[PLUGIN_COMMAND] == "leave":
                dest = RNS.Destination(
                    message.source.identity,
                    RNS.Destination.OUT,
                    RNS.Destination.SINGLE,
                    "lxmf",
                    "delivery",
                )
                self.connections.pop(dest.identity.hash, None)
                RNS.log(f"Connection removed: {message.source}")
                confirmation = LXMF.LXMessage(
                    dest,
                    self.my_lxmf_dest,
                    "Connection removed",
                    desired_method=LXMF.LXMessage.DIRECT,
                )
                self.lxm_router.handle_outbound(confirmation)
                continue
            msg = self.tel_controller.handle_command(
                command, message, self.my_lxmf_dest
            )
            if msg:
                self.lxm_router.handle_outbound(msg)

    def delivery_callback(self, message: LXMF.LXMessage):
        """Callback function to handle incoming messages.

        Args:
            message (LXMF.LXMessage): LXMF message object
        """
        try:
            # Format the timestamp of the message
            time_string = time.strftime(
                "%Y-%m-%d %H:%M:%S", time.localtime(message.timestamp)
            )
            signature_string = "Signature is invalid, reason undetermined"

            # Determine the signature validation status
            if message.signature_validated:
                signature_string = "Validated"
            elif message.unverified_reason == LXMF.LXMessage.SIGNATURE_INVALID:
                signature_string = "Invalid signature"
                return
            elif message.unverified_reason == LXMF.LXMessage.SOURCE_UNKNOWN:
                signature_string = "Cannot verify, source is unknown"
                return

            # Log the delivery details
            self.log_delivery_details(message, time_string, signature_string)

            # Handle the commands
            if message.signature_validated and LXMF.FIELD_COMMANDS in message.fields:
                self.command_handler(message.fields[LXMF.FIELD_COMMANDS], message)

            # Handle telemetry data
            if self.tel_controller.handle_message(message):
                RNS.log("Telemetry data saved")

            # Skip if the message content is empty
            if message.content is None or message.content == b"":
                return

            # Broadcast the message to all connected clients
            msg = (
                self.identities[message.get_source().hash]
                + " > "
                + message.content_as_string()
            )
            self.send_message(msg)
        except Exception as e:
            RNS.log(f"Error: {e}")

    def send_message(self, message: str):
        """Sends a message to all connected clients.

        Args:
            message (str): Message to send
        """
        for connection in self.connections:
            response = LXMF.LXMessage(
                connection,
                self.my_lxmf_dest,
                message,
                desired_method=LXMF.LXMessage.DIRECT,
            )
            self.lxm_router.handle_outbound(response)

    def log_delivery_details(self, message, time_string, signature_string):
        RNS.log("\t+--- LXMF Delivery ---------------------------------------------")
        RNS.log(f"\t| Source hash            : {RNS.prettyhexrep(message.source_hash)}")
        RNS.log(f"\t| Source instance        : {message.get_source()}")
        RNS.log(
            f"\t| Destination hash       : {RNS.prettyhexrep(message.destination_hash)}"
        )
        # RNS.log(f"\t| Destination identity   : {message.source_identity}")
        RNS.log(f"\t| Destination instance   : {message.get_destination()}")
        RNS.log(f"\t| Transport Encryption   : {message.transport_encryption}")
        RNS.log(f"\t| Timestamp              : {time_string}")
        RNS.log(f"\t| Title                  : {message.title_as_string()}")
        RNS.log(f"\t| Content                : {message.content_as_string()}")
        RNS.log(f"\t| Fields                 : {message.fields}")
        RNS.log(f"\t| Message signature      : {signature_string}")
        RNS.log("\t+---------------------------------------------------------------")

    def load_or_generate_identity(self, identity_path):
        # Load existing identity or generate a new one
        if os.path.exists(identity_path):
            try:
                RNS.log("Loading existing identity")
                return RNS.Identity.from_file(identity_path)
            except:
                RNS.log("Failed to load existing identity, generating new")
        else:
            RNS.log("Generating new identity")

        identity = RNS.Identity()  # Create a new identity
        Path(identity_path).parent.mkdir(parents=True, exist_ok=True)
        identity.to_file(identity_path)  # Save the new identity to file
        return identity

    def interactive_loop(self):
        # Periodically announce the LXMF identity
        while True:
            choice = input("Enter your choice (exit/announce/telemetry): ")

            if choice == "exit":
                break
            elif choice == "announce":
                self.my_lxmf_dest.announce()
            elif choice == "telemetry":
                connection_hash = input("Enter the connection hash: ")
                found = False
                for connection in self.connections:
                    if connection.hexhash == connection_hash:
                        message = LXMF.LXMessage(
                            connection,
                            self.my_lxmf_dest,
                            "Requesting telemetry",
                            desired_method=LXMF.LXMessage.DIRECT,
                            fields={
                                LXMF.FIELD_COMMANDS: [
                                    {TelemetryController.TELEMETRY_REQUEST: 1000000000}
                                ]
                            },
                        )
                        self.lxm_router.handle_outbound(message)
                        found = True
                        break
                if not found:
                    print("Connection not found")
    def headless_loop(self):
        while True:
            self.my_lxmf_dest.announce()
            time.sleep(60)

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "-s", "--storage_dir", help="Storage directory path", default=STORAGE_PATH
    )
    ap.add_argument("--headless", action="store_true", help="Run in headless mode")
    ap.add_argument("--display_name", help="Display name for the server", default="RTH")

    args = ap.parse_args()

    if args.storage_dir:
        storage_path = args.storage_dir
        identity_path = os.path.join(STORAGE_PATH, "identity")

    reticulum_server = ReticulumTelemetryHub(
        args.display_name, storage_path, identity_path
    )

    if not args.headless:
        reticulum_server.interactive_loop()
    else:
        reticulum_server.headless_loop()
