from .sensor_enum import *
from .location import Location
from .time import Time
from .magnetic_field import MagneticField
sid_mapping = {
    SID_LOCATION: Location,
    SID_TIME: Time,
    SID_MAGNETIC_FIELD: MagneticField,
}
