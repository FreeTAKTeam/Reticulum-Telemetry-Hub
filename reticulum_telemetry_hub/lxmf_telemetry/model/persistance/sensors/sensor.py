from sqlalchemy import Column, ForeignKey, Integer, Float, Boolean, String, create_engine, BLOB
from msgpack import packb, unpackb
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker, relationship, Mapped, mapped_column
import time
from typing import TYPE_CHECKING
from .. import Base

class Sensor(Base):
    __tablename__ = 'Sensor'

    id = Column(Integer, primary_key=True, autoincrement=True)
    sid = Column(Integer, nullable=False, default=0x00)
    stale_time = Column(Float, nullable=True)
    data = Column(BLOB, nullable=True)
    synthesized = Column(Boolean, default=False)
    telemeter_id: Mapped[int] = mapped_column(ForeignKey('Telemeter.id'))
    telemeter = relationship("Telemeter", back_populates='sensors')

    def __init__(self, stale_time=None, data=None, active=False, synthesized=False, last_update=0, last_read=0):
        self.stale_time = stale_time
        self.data = data
        self.active = active
        self.synthesized = synthesized
        self.last_update = last_update
        self.last_read = last_read

    def packb(self):
        return packb(self.pack())

    def unpackb(self, packed):
        return unpackb(self.unpack(packed))

    def pack(self):
        return self.data

    def unpack(self, packed):
        return packed

    __mapper_args__ = {
        'polymorphic_identity': 'Sensor',
        'with_polymorphic': '*',
        "polymorphic_on": sid
    }