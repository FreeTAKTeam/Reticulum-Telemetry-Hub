from sqlalchemy import Column
from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.sensors.sensor import Sensor
from .sensor_enum import SID_MAGNETIC_FIELD
import struct
import RNS
from sqlalchemy import Integer, ForeignKey, Float, DateTime
from sqlalchemy.orm import Mapped, mapped_column
from datetime import datetime

class MagneticField(Sensor):
    __tablename__ = 'MagneticField'

    id: Mapped[int] = mapped_column(ForeignKey('Sensor.id'), primary_key=True)
    x: Mapped[float] = mapped_column()
    y: Mapped[float] = mapped_column()
    z: Mapped[float] = mapped_column()

    def __init__(self):
        super().__init__(stale_time=15)
        self.x = None
        self.y = None
        self.z = None

    def pack(self):
        return [self.x, self.y, self.z]

    def unpack(self, packed):
        try:
            return {"x": packed[0], "y": packed[1], "z": packed[2]}
        except:
            return None

    __mapper_args__ = {
        'polymorphic_identity': SID_MAGNETIC_FIELD,
        'with_polymorphic': '*'
    }
