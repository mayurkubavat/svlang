// SVL-MNT-044 good: prose comments, even ones that mention code.
class bus_driver;
  int unsigned sent;
  task drive();
    // Reset the counter here; the monitor reads it at the end of each burst;
    // keep this before the first clock edge.
    sent = 0;
    // sent++;
  endtask
endclass
