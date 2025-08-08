from os import getenv
from dotenv import load_dotenv
from requests import get
from LabelWindow import LabelWindow

load_dotenv()

API_URL = getenv('JAHAN_NAMA_API_URL')
INTERVAL = getenv('JAHAN_NAMA_INTERVAL_SECONDS')
INTERVAL = int(INTERVAL) * 1_000


def get_remaining_data() -> str:
    try:
        return get(API_URL).json().get('data', {}).get('ترافیک جاری', '0')
    except Exception as exception:
        print(f"Error: {str(exception)}")
    return '-'

class App(LabelWindow):

    def text_schedule(self):
        remaining = get_remaining_data()
        self.set_label_text(remaining)

if __name__ == "__main__":
    app = App()
    app.set_label_font('IRANSansWeb')
    app.set_interval(INTERVAL)
    app.mainloop()
