from launch import LaunchDescription
from launch_ros.actions import Node

def generate_launch_description():
    return LaunchDescription([
        Node(
            package="my_test",
            executable="talker",
            name="talker",
        ),
        Node(
            package="my_test",
            executable="listener",
            name="listener",
        )
    ])
