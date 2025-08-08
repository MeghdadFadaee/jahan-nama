import time
import tkinter as tk


class LabelWindow(tk.Tk):
    def __init__(self):
        super().__init__()
        self._drag_data = None
        self.overrideredirect(True)  # Remove window decorations
        self.attributes('-transparentcolor', 'black')  # Make the white color transparent
        self.configure(bg='black')  # Set the background color of the window
        self.attributes('-topmost', True)  # Keep the window on top of all others

        screen_width = self.winfo_screenwidth()
        screen_height = self.winfo_screenheight()

        window_width = 130
        window_height = 30

        # Set the position of the window to the top right corner
        self.geometry(f'+{screen_width - window_width}+0')
        self.geometry(f'{window_width}x{window_height}')  # Set the size of the window

        self.click_count = 0  # Counter for right clicks
        self.last_click_time = 0  # Time of the last right click
        self.interval = 1000  # Interval in milliseconds
        self.base_color = 'darkgreen'  # base color for label
        self.hover_color = 'lightgreen'  # hovered color for label

        # Create a label with the given text
        self.label = tk.Label(self,
                              bg="black",
                              fg=self.base_color,
                              text="0",
                              font=("Comic Sans MS", 14))
        self.label.pack(expand=True)

        # Bind the mouse events for dragging
        self.label.bind('<Button-1>', self.start_drag)
        self.label.bind('<B1-Motion>', self.do_drag)
        self.label.bind('<Button-3>', self.right_click)  # Bind right click event

        # Bind the mouse events for changing color
        self.label.bind('<Enter>', self.on_enter)
        self.label.bind('<Leave>', self.on_leave)

        # Schedule the text change
        self.schedule()

    def start_drag(self, event):
        self._drag_data = {'x': event.x, 'y': event.y}

    def do_drag(self, event):
        deltaX = event.x - self._drag_data['x']
        deltaY = event.y - self._drag_data['y']
        x = self.winfo_x() + deltaX
        y = self.winfo_y() + deltaY
        self.geometry(f'+{x}+{y}')

    def right_click(self, event):
        current_time = time.time()
        # Check if the time between clicks is less than 2 second
        if current_time - self.last_click_time < 2:
            self.click_count += 1
        else:
            self.click_count = 1
        self.last_click_time = current_time

        if self.click_count >= 3:
            self.destroy()

    def on_enter(self, event):
        self.label.config(fg=self.hover_color)  # Change text color to hover color

    def on_leave(self, event):
        self.label.config(fg=self.base_color)  # Change text color back to base color

    def set_label_font(self, name: str, size: int = 14) -> None:
        self.label.config(font=(name, size))  # Change label font

    def set_label_text(self, text=None) -> None:
        if text is not None:
            self.label.config(text=text)  # Change label text

    def set_interval(self, new_interval: int) -> None:
        if isinstance(new_interval, int):
            self.interval = new_interval  # Change app interval

    def schedule(self):
        self.text_schedule()  # add Schedule
        self.after(self.interval, self.schedule)  # Schedule the next text change

    def text_schedule(self):
        pass


if __name__ == "__main__":
    app = LabelWindow()
    app.set_label_text('1000')
    app.mainloop()
