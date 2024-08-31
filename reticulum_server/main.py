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
from lxmf_telemetry.telemetry_controller import TelemetryController

# Constants
STORAGE_PATH = "./tmp1"  # Path to store temporary files
IDENTITY_PATH = os.path.join(STORAGE_PATH, "identity")  # Path to store identity file
APP_NAME = LXMF.APP_NAME + ".delivery"  # Application name for LXMF

tel_controller = TelemetryController()

class AnnounceHandler:
    def __init__(self, connections, my_lxmf_dest, lxm_router):
        self.aspect_filter = APP_NAME  # Filter for LXMF announcements
        self.connections = connections  # List to store connections
        self.my_lxmf_dest = my_lxmf_dest  # LXMF destination
        self.lxm_router = lxm_router  # LXMF router

    def received_announce(self, destination_hash, announced_identity, app_data):
        # Log the received announcement details
        RNS.log("\t+--- LXMF Announcement -----------------------------------------")
        RNS.log(f"\t| Source hash            : {RNS.prettyhexrep(destination_hash)}")
        RNS.log(f"\t| Announced identity     : {announced_identity}")
        RNS.log(f"\t| App data               : {app_data}")
        RNS.log("\t+---------------------------------------------------------------")

        # Create a new destination from the announced identity
        dest = RNS.Destination(
            announced_identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "lxmf",
            "delivery",
        )
        self.connections.append(dest)  # Add the new destination to connections

        # Create and send a message to the new destination
        message = LXMF.LXMessage(
            dest, self.my_lxmf_dest, "Hi there", desired_method=LXMF.LXMessage.DIRECT
        )
        self.lxm_router.handle_outbound(message)  # Handle outbound message

def command_handler(commands: list, message: LXMF.LXMessage, lxm_router, my_lxmf_dest):
    for command in commands:
        print(f"Command: {command}")
        msg = tel_controller.handle_command(command, message, my_lxmf_dest)
        if msg:
            lxm_router.handle_outbound(msg)

def delivery_callback(message: LXMF.LXMessage, connections, my_lxmf_dest, lxm_router):
    # Format the timestamp of the message
    try:
        time_string = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(message.timestamp))
        signature_string = "Signature is invalid, reason undetermined"

        # Determine the signature validation status
        if message.signature_validated:
            signature_string = "Validated"
        elif message.unverified_reason == LXMF.LXMessage.SIGNATURE_INVALID:
            signature_string = "Invalid signature"
        elif message.unverified_reason == LXMF.LXMessage.SOURCE_UNKNOWN:
            signature_string = "Cannot verify, source is unknown"

        if message.signature_validated and LXMF.FIELD_COMMANDS in message.fields:
            command_handler(message.fields[LXMF.FIELD_COMMANDS], message, lxm_router, my_lxmf_dest)
            return
        
        if tel_controller.handle_message(message):
            RNS.log("Telemetry data saved")
            return

        # Log the delivery details
        RNS.log("\t+--- LXMF Delivery ---------------------------------------------")
        RNS.log(f"\t| Source hash            : {RNS.prettyhexrep(message.source_hash)}")
        RNS.log(f"\t| Source instance        : {message.get_source()}")
        RNS.log(
            f"\t| Destination hash       : {RNS.prettyhexrep(message.destination_hash)}"
        )
        #RNS.log(f"\t| Destination identity   : {message.source_identity}")
        RNS.log(f"\t| Destination instance   : {message.get_destination()}")
        RNS.log(f"\t| Transport Encryption   : {message.transport_encryption}")
        RNS.log(f"\t| Timestamp              : {time_string}")
        RNS.log(f"\t| Title                  : {message.title_as_string()}")
        RNS.log(f"\t| Content                : {message.content_as_string()}")
        RNS.log(f"\t| Fields                 : {message.fields}")
        RNS.log(f"\t| Message signature      : {signature_string}")
        RNS.log("\t+---------------------------------------------------------------")
        for connection in connections:
            if connection.hash != message.source_hash:
                response = LXMF.LXMessage(
                    connection,
                    my_lxmf_dest,
                    message.content_as_string(),
                    desired_method=LXMF.LXMessage.DIRECT,
                )
                lxm_router.handle_outbound(response)  # Handle outbound response
    except Exception as e:
        RNS.log(f"Error: {e}")

def load_or_generate_identity(identity_path):
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
    identity.to_file(identity_path)  # Save the new identity to file
    return identity



def main():
    global my_lxmf_dest
    connections = []  # List to store connections
    r = RNS.Reticulum()  # Initialize Reticulum
    lxm_router = LXMF.LXMRouter(storagepath=STORAGE_PATH)  # Initialize LXMF router
    # lxm_router.enable_propagation()
    identity = load_or_generate_identity(IDENTITY_PATH)  # Load or generate identity
    my_lxmf_dest = lxm_router.register_delivery_identity(
        identity, "test_server"
    )  # Register delivery identity
    lxm_router.set_message_storage_limit(megabytes=5)
    lxm_router.enable_propagation()

    # Register delivery callback
    lxm_router.register_delivery_callback(
        lambda msg: delivery_callback(msg, connections, my_lxmf_dest, lxm_router)
    )
    # Register announce handler
    RNS.Transport.register_announce_handler(
        AnnounceHandler(connections, my_lxmf_dest, lxm_router)
    )

    # Announce LXMF identity
    my_lxmf_dest.announce()
    lxm_router.announce_propagation_node()
    RNS.log("LXMF identity announced")
    RNS.log("\t+--- LXMF Identity ---------------------------------------------")
    RNS.log(f"\t| Hash                   : {RNS.prettyhexrep(my_lxmf_dest.hash)}")
    RNS.log(
        f"\t| Public key             : {RNS.prettyhexrep(my_lxmf_dest.identity.pub.public_bytes())}"
    )
    RNS.log("\t+---------------------------------------------------------------")

    # Periodically announce the LXMF identity
    while True:
        choice = input("Enter your choice (exit/announce/telemetry): ")
        
        if choice == "exit":
            break
        elif choice == "announce":
            my_lxmf_dest.announce()
        elif choice == "announce_prop":
            lxm_router.announce_propagation_node()
        elif choice == "telemetry":
            connection_hash = input("Enter the connection hash: ")
            found = False
            for connection in connections:
                if connection.hexhash == connection_hash:
                    message = LXMF.LXMessage(
                    connection,
                    my_lxmf_dest,
                    "Requesting telemetry",
                    desired_method=LXMF.LXMessage.DIRECT,
                    fields={LXMF.FIELD_COMMANDS: [{TelemetryController.TELEMETRY_REQUEST: 1000000000}]}
                    )
                    lxm_router.handle_outbound(message)
                    found = True
                    break
            if not found:
                print("Connection not found")


if __name__ == "__main__":
    main()
