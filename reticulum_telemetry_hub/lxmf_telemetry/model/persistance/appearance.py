from sqlalchemy import Column, Integer, String, ForeignKey, BLOB
from sqlalchemy.orm import relationship
from . import Base


class Appearance(Base):

    __tablename__ = "Appearance"

    id = Column(Integer, primary_key=True, autoincrement=True)
    icon = Column(String, nullable=False)
    foreground = Column(String, nullable=False)
    background = Column(String, nullable=False)
    peer_id = Column(String, ForeignKey("Peer.destination_hash"))
    peer = relationship("Peer", back_populates="appearance")

    def __init__(self, peer, icon = "Default"):
        self.peer = peer
        self.icon = icon
