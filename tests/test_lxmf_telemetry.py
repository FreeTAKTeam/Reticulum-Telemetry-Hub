from lxmf_telemetry.telemetry_controller import TelemetryController

def test_deserialize_lxmf():
    with open('sample.bin', 'rb') as f:
        data = f.read()
    
    tel = TelemetryController()._deserialize_telemeter(data)
    assert tel.sensors[0].latitude == 44.657059
    assert True