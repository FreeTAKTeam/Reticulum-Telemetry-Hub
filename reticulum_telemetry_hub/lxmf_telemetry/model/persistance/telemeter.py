from typing import TYPE_CHECKING, Optional
from . import Base
from sqlalchemy import Column, Integer, DateTime, String, ForeignKey
from sqlalchemy.orm import relationship, Mapped, mapped_column
from datetime import datetime
from msgpack import packb, unpackb

if TYPE_CHECKING:
    from .sensors.sensor import Sensor

class Telemeter(Base):
    __tablename__ = "Telemeter"
    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    time: Mapped[datetime] = mapped_column(DateTime, nullable=False)

    sensors: Mapped[list["Sensor"]] = relationship("Sensor", back_populates="telemeter")

    peer_dest: Mapped[str] = mapped_column(String, nullable=False) # mapped_column(ForeignKey("Peer.destination_hash"))
    #peer = relationship("Peer", back_populates="telemeters")

    def __init__(self, peer_dest: str, time: Optional[datetime] = None):
        self.peer_dest = peer_dest
        self.time = time or datetime.now()
