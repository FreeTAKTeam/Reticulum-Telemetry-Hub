from datetime import datetime
from typing import Optional

from sqlalchemy import ForeignKey, DateTime
from sqlalchemy.orm import Mapped, mapped_column

from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.sensors.sensor_enum import SID_TIME
from reticulum_telemetry_hub.lxmf_telemetry.model.persistance.sensors.sensor import Sensor

class Time(Sensor):
    __tablename__ = 'Time'

    id: Mapped[int] = mapped_column(ForeignKey('Sensor.id'), primary_key=True)
    utc: Mapped[datetime] = mapped_column(DateTime)

    def __init__(self, utc: Optional[datetime] = None):
        super().__init__(stale_time=15)
        self.utc = utc or datetime.now()

    def pack(self):
        return self.utc.timestamp()
    
    def unpack(self, packed):
        if packed is None:
            return None
        else:
            self.utc = datetime.fromtimestamp(packed)

    __mapper_args__ = {
        'polymorphic_identity': SID_TIME,
        'with_polymorphic': '*'
    }