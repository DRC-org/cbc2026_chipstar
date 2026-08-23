from launch import LaunchDescription
from launch_ros.actions import Node

def generate_launch_description():
    return LaunchDescription([
        Node(
            package="my_test",
            executable="add_two_ints_server",
            name="add_two_ints_server",
        ),
        Node(
            package="my_test",
            executable="add_two_ints_client",
            name="add_two_ints_client",
        )
    ])
