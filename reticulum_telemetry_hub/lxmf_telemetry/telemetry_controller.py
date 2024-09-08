from typing import Optional
from datetime import datetime
import LXMF
import RNS
from msgpack import packb, unpackb
from reticulum_telemetry_hub.lxmf_telemetry.model.persistance import Base
from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.sensors.sensor import Sensor
from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.telemeter import Telemeter

from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.sensors.sensor_mapping import sid_mapping
from sqlalchemy import create_engine
from sqlalchemy.orm import sessionmaker, Session, joinedload

_engine = create_engine("sqlite:///telemetry.db")
Base.metadata.create_all(_engine)
Session_cls = sessionmaker(bind=_engine)


class TelemetryController:
    """This class is responsible for managing the telemetry data."""

    TELEMETRY_REQUEST = 1

    def __init__(self) -> None:
        pass

    def get_telemetry(
        self, start_time: Optional[datetime] = None, end_time: Optional[datetime] = None
    ) -> list[Telemeter]:
        """Get the telemetry data."""
        with Session_cls() as ses:
            query = ses.query(Telemeter)
            if start_time:
                query = query.filter(Telemeter.time >= start_time)
            if end_time:
                query = query.filter(Telemeter.time <= end_time)
            tels = query.options(joinedload(Telemeter.sensors)).all()
            return tels

    def save_telemetry(self, telemetry_data: dict, peer_dest) -> None:
        """Save the telemetry data."""
        tel = self._deserialize_telemeter(telemetry_data, peer_dest)
        with Session_cls() as ses:
            ses.add(tel)
            ses.commit()

    def handle_message(self, message: LXMF.LXMessage) -> bool:
        """Handle the incoming message."""
        handled = False
        if LXMF.FIELD_TELEMETRY in message.fields:
            tel_data: dict = unpackb(
                message.fields[LXMF.FIELD_TELEMETRY], strict_map_key=False
            )
            RNS.log(f"Telemetry data: {tel_data}")
            self.save_telemetry(tel_data, RNS.hexrep(message.source_hash, False))
            handled = True
        if LXMF.FIELD_TELEMETRY_STREAM in message.fields:
            tels_data = unpackb(
                message.fields[LXMF.FIELD_TELEMETRY_STREAM], strict_map_key=False
            )
            for tel_data in tels_data:
                self.save_telemetry(tel_data, RNS.hexrep(tel_data.pop(0)))
            handled = True

        return handled

    def handle_command(self, command: dict, message: LXMF.LXMessage, my_lxm_dest) -> Optional[LXMF.LXMessage]:
        """Handle the incoming command."""
        if TelemetryController.TELEMETRY_REQUEST in command:
            timebase = command[TelemetryController.TELEMETRY_REQUEST]
            tels = self.get_telemetry(start_time=datetime.fromtimestamp(timebase))
            packed_tels = []
            dest = RNS.Destination(
                message.source.identity,
                RNS.Destination.OUT,
                RNS.Destination.SINGLE,
                "lxmf",
                "delivery",
            )
            message = LXMF.LXMessage(
                    dest,
                    my_lxm_dest,
                    "Telemetry data",
                    desired_method=LXMF.LXMessage.DIRECT,
                )
            for tel in tels:
                tel_data = self._serialize_telemeter(tel)
                packed_tels.append(
                    [
                        bytes.fromhex(tel.peer_dest),
                        round(tel.time.timestamp()),
                        packb(tel_data),
                        ['account', b'\x00\x00\x00', b'\xff\xff\xff'],
                    ]
                )
            message.fields[LXMF.FIELD_TELEMETRY_STREAM] = packed_tels
            print("+--- Sending telemetry data---------------------------------")
            print(f"| Telemetry data: {packed_tels}")
            print(f"| Message: {message}")
            print("+------------------------------------------------------------")
            return message
        else:
            return None

    def _serialize_telemeter(self, telemeter: Telemeter) -> dict:
        """Serialize the telemeter data."""
        telemeter_data = {}
        for sensor in telemeter.sensors:
            sensor_data = sensor.pack()
            telemeter_data[sensor.sid] = sensor_data
        return telemeter_data

    def _deserialize_telemeter(self, tel_data: dict, peer_dest: str) -> Telemeter:
        """Deserialize the telemeter data."""
        tel = Telemeter(peer_dest)
        for sid in tel_data:
            if sid in sid_mapping:
                if tel_data[sid] is None:
                    RNS.log(f"Sensor data for {sid} is None")
                    continue
                sensor = sid_mapping[sid]()
                sensor.unpack(tel_data[sid])
                tel.sensors.append(sensor)
        return tel
