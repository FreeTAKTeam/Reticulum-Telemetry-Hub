# Reticulum-Telemetry-Hub (RTH)
![image](https://github.com/user-attachments/assets/ba29799c-7194-4052-aedf-1b5e1c8648d5)


Reticulum-Telemetry-Hub (RTH) is an independent component within the [Reticulum](https://reticulum.network/) / [lXMF](https://github.com/markqvist/LXMF) ecosystem, designed to manage a complete TCP node across a Reticulum-based network. 
The RTH  enable communication and data sharing between clients like [Sideband](https://github.com/FreeTAKTeam/Sideband](https://github.com/markqvist/Sideband)) or Meshchat, enhancing situational awareness and operational efficiency in distributed networks.

## Core Functionalities

The Reticulum-Telemetry-Hub can perform the following key functions:

- **One to Many Messages**: RTH supports broadcasting messages to all connected clients.
- By sending a message to the hub, it will be distributed to all clients connected to the network. *(Initial implementation - Experimental)*
- **Telemetry Collector**: RTH acts as a telemetry data repository, collecting data from all connected clients.
  Currently, this functionality is focused on Sideband clients that have enabled their Reticulum identity. By  rewriting the code we hope to see a wider implementation of Telemetry in other applications. 
- **Replication Node**: RTH uses the LXMF router to ensure message delivery even when the target client is offline. If a message's destination is not available at the time of sending, RTH will save the message and deliver it once the client comes online.
- **Reticulum Transport**: RTH uses Reticulum  as a transport node, routing traffic to other peers, passing network announcements, and fulfilling path requests.

## Installation
To install Reticulum-Telemetry-Hub, clone the repository and proceed with the following steps:

```bash
git clone https://github.com/FreeTAKTeam/Reticulum-Telemetry-Hub.git
cd Reticulum-Telemetry-Hub
```

## Configuration
until we implement the wizard you will need to configure different config files.
## RNS Config file
located under ```/[USERNAME]/.reticulum```
```
[reticulum]  
  enable_transport = True
    share_instance = Yes
[interfaces]
  	
  [[TCP Server Interface]]
  type = TCPServerInterface
  interface_enabled = True

  # This configuration will listen on all IP
  # interfaces on port 4242

  listen_ip = 0.0.0.0
  listen_port = 4242
```
## Router Config File
located under ```/[USERNAME]/.lxmd``` 
```
[propagation]
enable_node = yes
# Automatic announce interval in minutes, suggested.
announce_interval = 10
propagation_transfer_max_accepted_size = 1024

[lxmf]
display_name = RTH_router

```

## Service
In order to start the router  automatically on startup, we will need to install a /etc/systemd/system/lxmd.service file:

```
[Unit]
Description=Reticulum LXMF Daemon (lxmd)
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/lxmd
Restart=on-failure
User=root  # Change this if you run lxmd as a non-root user
WorkingDirectory=/usr/local/bin  # Adjust to where lxmd is located
ExecReload=/bin/kill -HUP $MAINPID

[Install]
WantedBy=multi-user.target
```

## Usage
Enable and start the service: Once the service file is created, run the following commands to enable and start the service:

```bash
Copy code
sudo systemctl daemon-reload
sudo systemctl enable lxmd.service
sudo systemctl start lxmd.service
```

Ensure your Reticulum network  is operational and configure for the full functionality of RTH.
Once installed and configured, you can start the Reticulum-Telemetry-Hub by running:

```bash
python3 main.py
```



### Project Roadmap
- **Transition to Command-Based Server Joining**: Shift the "joining the server" functionality from an announce-based method to a command-based approach for improved control and scalability.
  - **Object-Based Configuration Management**: Refactor the system to enable access to all configuration files via objects, enhancing modularity and ease of management.
- **Configuration Wizard Development**: Introduce a user-friendly wizard to simplify the configuration process.
- **Integration with TAK_LXMF Bridge**: Incorporate RTH into the TAK_LXMF bridge to strengthen the link between TAK devices and Reticulum networks.
- **Foundation for FTS "Flock of Parrot"**: Use RTH as the base for implementing the FreeTAKServer "Flock of Parrot" concept, aiming for scalable, interconnected FTS instances.

## Contributing
We welcome and encourage contributions from the community! To contribute, please fork the repository and submit a pull request. Make sure that your contributions adhere to the project's coding standards and include appropriate tests.

## License
This project is licensed under the Creative Commons License Attribution-NonCommercial-ShareAlike 4.0 International. For more details, refer to the `LICENSE` file in the repository.

## Support
For any issues or support, feel free to open an issue on this GitHub repository or join the FreeTAKServer community on [Discord](The FTS Discord Server).

# Support Reticulum
You can help support the continued development of open, free and private communications systems by donating via one of the following channels to the original Reticulm author:

* Monero: 84FpY1QbxHcgdseePYNmhTHcrgMX4nFfBYtz2GKYToqHVVhJp8Eaw1Z1EedRnKD19b3B8NiLCGVxzKV17UMmmeEsCrPyA5w
* Ethereum: 0xFDabC71AC4c0C78C95aDDDe3B4FA19d6273c5E73
* Bitcoin: 35G9uWVzrpJJibzUwpNUQGQNFzLirhrYAH
* Ko-Fi: https://ko-fi.com/markqvist
