import os
from requests import Session
from dotenv import load_dotenv
from bs4 import BeautifulSoup
from bs4.element import Tag

load_dotenv()

username = os.getenv('JAHAN_NAMA_USERNAME')
password = os.getenv('JAHAN_NAMA_PASSWORD')

baseurl = 'https://qom.jahan-nama.com'
login_uri = '/user/signin?isSignout=1'
auth_uri = '/User/SigninCheck'


def pluck(data: list[dict], value: str, key: None | str) -> list | dict:
    if key is None:
        return [d.get(value) for d in data]

    plucked = {d.get(key, '__None'): d.get(value) for d in data}
    plucked.pop('__None', None)
    return plucked


def prettify_tag(tag: Tag) -> str:
    return tag.get_text().strip().strip(':').strip()


def tag_to_dict(tag: Tag) -> dict:
    return dict(tag.attrs)


def force_to_login(session: Session) -> Session:
    response = session.get(baseurl + login_uri)
    soup = BeautifulSoup(response.text, 'html.parser')
    inputs = soup.find_all('input')
    inputs = list(map(tag_to_dict, inputs))
    inputs = pluck(inputs, 'value', 'name')
    inputs.update({
        'Username': username,
        'Password': password,
    })

    authed_response = session.post(baseurl + auth_uri, data=inputs)
    return session


def get_information(session: Session) -> dict[str, str]:
    authed_response = session.get(baseurl)

    authed_soup = BeautifulSoup(authed_response.text, 'html.parser')

    information = authed_soup.find('div', class_='m-box box-info m-appendage m-bg-pink').find_all('label')
    information = list(map(prettify_tag, information))

    information = {information[i]: information[i + 1] for i in range(0, len(information), 2)}

    return information


def main():
    session = Session()

    session = force_to_login(session)
    information = get_information(session)

    for key, val in information.items():
        print(key, val)

    print('-' * 30)
    print(information['ترافیک جاری'])


if __name__ == '__main__':
    main()
