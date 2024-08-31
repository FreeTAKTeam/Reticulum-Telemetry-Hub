from . import Base
from typing import TYPE_CHECKING, Optional
from sqlalchemy import Column, Integer, String
from sqlalchemy.orm import Mapped, mapped_column, relationship
if TYPE_CHECKING:
    from .telemeter import Telemeter

class Peer(Base):
    __tablename__ = 'Peer'

    destination_hash: Mapped[str] = mapped_column(String, nullable=False, primary_key=True)
    telemeters = relationship("Telemeter")
    #appearance = relationship("Appearance", back_populates='peer')