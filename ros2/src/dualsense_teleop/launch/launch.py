from launch import LaunchDescription
from launch.actions import DeclareLaunchArgument
from launch.substitutions import LaunchConfiguration
from launch_ros.actions import Node


def generate_launch_description():
    return LaunchDescription(
        [
            DeclareLaunchArgument("joy_device", default_value="/dev/input/js0"),
            DeclareLaunchArgument("serial_device", default_value="/dev/ttyACM0"),
            Node(
                package="joy",
                executable="joy_node",
                name="joy_node",
                parameters=[
                    {
                        "device": LaunchConfiguration("joy_device"),
                        "deadzone": 0.05,
                        "autorepeat_rate": 20.0,
                    }
                ],
            ),
            Node(
                package="dualsense_teleop",
                executable="dualsense_teleop",
                name="dualsense_teleop",
            ),
            Node(
                package="dualsense_teleop",
                executable="cctl_usb_cdc_bridge",
                name="cctl_usb_cdc_bridge",
                parameters=[
                    {
                        "serial_device": LaunchConfiguration("serial_device"),
                        "controller_input_topic": "controller_input",
                    }
                ],
            ),
        ]
    )
