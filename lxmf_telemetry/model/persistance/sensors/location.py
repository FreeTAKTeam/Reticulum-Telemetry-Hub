from sqlalchemy import Column
from lxmf_telemetry.model.persistance.sensors.sensor import Sensor
from .sensor_enum import SID_LOCATION
import struct
import RNS
from sqlalchemy import Integer, ForeignKey, Float, DateTime
from sqlalchemy.orm import Mapped, mapped_column
from datetime import datetime

class Location(Sensor):
    __tablename__ = 'Location'

    id: Mapped[int] = mapped_column(ForeignKey('Sensor.id'), primary_key=True)
    latitude: Mapped[float] = mapped_column()
    longitude: Mapped[float] = mapped_column()
    altitude: Mapped[float] = mapped_column()
    speed: Mapped[float] = mapped_column()
    bearing: Mapped[float] = mapped_column()
    accuracy: Mapped[float] = mapped_column()
    last_update: Mapped[datetime] = mapped_column(DateTime)

    def __init__(self):
        super().__init__(stale_time=15)
        self.latitude = None
        self.longitude = None
        self.altitude = None
        self.speed = None
        self.bearing = None
        self.accuracy = None

    def pack(self):
        try:
            return [
                struct.pack("!i", int(round(self.latitude, 6) * 1e6)),
                struct.pack("!i", int(round(self.longitude, 6) * 1e6)),
                struct.pack("!I", int(round(self.altitude, 2) * 1e2)),
                struct.pack("!I", int(round(self.speed, 2) * 1e2)),
                struct.pack("!I", int(round(self.bearing, 2) * 1e2)),
                struct.pack("!H", int(round(self.accuracy, 2) * 1e2)),
                self.last_update.timestamp(),
            ]
        except (KeyError, ValueError, struct.error) as e:
            RNS.log(
                "An error occurred while packing location sensor data. "
                "The contained exception was: " + str(e),
                RNS.LOG_ERROR,
            )
            return None

    def unpack(self, packed):
        try:
            if packed is None:
                return None
            else:
                self.latitude = struct.unpack("!i", packed[0])[0] / 1e6
                self.longitude = struct.unpack("!i", packed[1])[0] / 1e6
                self.altitude = struct.unpack("!I", packed[2])[0] / 1e2
                self.speed = struct.unpack("!I", packed[3])[0] / 1e2
                self.bearing = struct.unpack("!I", packed[4])[0] / 1e2
                self.accuracy = struct.unpack("!H", packed[5])[0] / 1e2
                self.last_update = datetime.fromtimestamp(packed[6])
        except (struct.error, IndexError):
            return None


    __mapper_args__ = {
        'polymorphic_identity': SID_LOCATION,
        'with_polymorphic': '*'
    }
